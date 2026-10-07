use std::arch::x86_64::__cpuid;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};

use pulsar_core::sensors::{Publisher, Sensor, Status};
use windows::Win32::System::SystemInformation::GROUP_AFFINITY;

use crate::cores;
use crate::pawnio::{PawnIo, PciLock};

const INTEL_MODULE: &[u8] = include_bytes!("../../resources/pawnio/IntelMSR.bin");
const AMD_MODULE: &[u8] = include_bytes!("../../resources/pawnio/AMDFamily17.bin");
const INTEL_TEMPERATURE_TARGET: u32 = 0x1A2;
const INTEL_PACKAGE_THERM_STATUS: u32 = 0x1B1;
const INTEL_CORE_THERM_STATUS: u32 = 0x19C;
const INTEL_POWER_UNIT: u32 = 0x606;
const INTEL_PKG_ENERGY: u32 = 0x611;
const AMD_POWER_UNIT: u32 = 0xC001_0299;
const AMD_PKG_ENERGY: u32 = 0xC001_029B;
/// The thermal controller's current temperature, on the system management network.
const AMD_SMN_CUR_TEMP: u32 = 0x0005_9800;
const SAMPLE: Duration = Duration::from_secs(1);
/// How often to look for a driver that was missing or went away.
const RETRY: Duration = Duration::from_secs(10);
const STOP_POLL: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Vendor {
    Intel,
    Amd,
}

fn vendor_of(id: &[u8; 12]) -> Option<Vendor> {
    match id {
        b"GenuineIntel" => Some(Vendor::Intel),
        b"AuthenticAMD" => Some(Vendor::Amd),
        _ => None,
    }
}

fn vendor() -> Option<Vendor> {
    let leaf = __cpuid(0);
    let mut id = [0u8; 12];
    for (chunk, register) in id
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip([leaf.ebx, leaf.edx, leaf.ecx])
    {
        chunk.copy_from_slice(&register.to_le_bytes());
    }
    vendor_of(&id)
}

/// The processor's marketing name, e.g. `AMD Ryzen 7 2700X Eight-Core Processor`.
fn brand() -> String {
    let bytes: Vec<u8> = (0x8000_0002..=0x8000_0004u32)
        .flat_map(|leaf| {
            let r = __cpuid(leaf);
            [r.eax, r.ebx, r.ecx, r.edx]
        })
        .flat_map(u32::to_le_bytes)
        .take_while(|&b| b != 0)
        .collect();
    String::from_utf8_lossy(&bytes).trim().to_string()
}

/// Intel temperature in °C, for the package or a core: the CPU reports how
/// far it is below its throttling point, which the target register names.
fn intel_temp(target: u64, status: u64) -> f64 {
    let tj_max = match (target >> 16) & 0xFF {
        0 => 100,
        t => t as i32,
    };
    let below = ((status >> 16) & 0x7F) as i32;
    f64::from(tj_max - below)
}

/// Whether an Intel thermal status says the CPU is slowing itself down now.
fn intel_throttling(status: u64) -> bool {
    status & 1 != 0
}

/// A core's status carries a flag for whether its reading can be used.
fn intel_core_valid(status: u64) -> bool {
    status & (1 << 31) != 0
}

/// How far some AMD models' control temperature sits above the real one, to
/// make their fans spin up sooner.
fn amd_tctl_offset(brand: &str) -> f64 {
    const OFFSETS: [(&str, f64); 8] = [
        ("Ryzen 5 1600X", 20.0),
        ("Ryzen 7 1700X", 20.0),
        ("Ryzen 7 1800X", 20.0),
        ("Ryzen 7 2700X", 10.0),
        ("Threadripper 19", 27.0),
        ("Threadripper 29", 27.0),
        ("Threadripper 39", 0.0),
        ("Threadripper 59", 0.0),
    ];
    OFFSETS
        .iter()
        .find(|(model, _)| brand.contains(model))
        .map_or(0.0, |&(_, offset)| offset)
}

/// AMD die temperature in °C from the thermal controller's register: eighths
/// of a degree in the top bits, on a scale that starts at -49 °C when either
/// range flag is set.
fn amd_temp(raw: u32, tctl_offset: f64) -> f64 {
    let low_range = raw & (1 << 19) != 0 || (raw >> 16) & 0b11 == 0b11;
    let base = if low_range { -49.0 } else { 0.0 };
    f64::from(raw >> 21) / 8.0 + base - tctl_offset
}

/// Joules per count of the energy registers.
fn energy_unit(power_unit: u64) -> f64 {
    1.0 / f64::from(1u32 << ((power_unit >> 8) & 0x1F))
}

/// Average watts between two readings of a 32-bit energy counter that wraps.
fn watts(before: u64, after: u64, joules_per_count: f64, elapsed: Duration) -> Option<f64> {
    let counts = (after as u32).wrapping_sub(before as u32);
    let seconds = elapsed.as_secs_f64();
    (seconds > 0.0).then(|| f64::from(counts) * joules_per_count / seconds)
}

enum Thermal {
    Intel { cores: Vec<GROUP_AFFINITY> },
    Amd { tctl_offset: f64, pci: PciLock },
}

/// A loaded module and what the last sample left behind.
struct Session {
    io: PawnIo,
    thermal: Thermal,
    energy_msr: u32,
    /// Zero where the platform has no energy counters.
    joules_per_count: f64,
    energy: Option<(u64, Instant)>,
}

impl Session {
    fn connect() -> Result<Self, Status> {
        let vendor = vendor().ok_or(Status::UnsupportedCpu)?;
        let (module, unit_msr, energy_msr) = match vendor {
            Vendor::Intel => (INTEL_MODULE, INTEL_POWER_UNIT, INTEL_PKG_ENERGY),
            Vendor::Amd => (AMD_MODULE, AMD_POWER_UNIT, AMD_PKG_ENERGY),
        };
        let io = PawnIo::open().map_err(|_| Status::NoDriver)?;
        // The modules turn away families they do not know.
        io.load(module).map_err(|_| Status::UnsupportedCpu)?;
        let thermal = match vendor {
            Vendor::Intel => Thermal::Intel {
                cores: cores::physical_cores(),
            },
            Vendor::Amd => Thermal::Amd {
                tctl_offset: amd_tctl_offset(&brand()),
                pci: PciLock::open().map_err(|_| Status::ReadFailed)?,
            },
        };
        let joules_per_count = io.read_msr(unit_msr).map_or(0.0, energy_unit);
        Ok(Self {
            io,
            thermal,
            energy_msr,
            joules_per_count,
            energy: None,
        })
    }

    /// The package temperature, and on Intel whether it is throttling and
    /// each core's own temperature.
    fn thermal(&self) -> (Vec<(Sensor, f64)>, Vec<f64>) {
        match &self.thermal {
            Thermal::Intel { cores } => {
                let registers = self
                    .io
                    .read_msr(INTEL_TEMPERATURE_TARGET)
                    .and_then(|target| Ok((target, self.io.read_msr(INTEL_PACKAGE_THERM_STATUS)?)));
                let Ok((target, status)) = registers else {
                    return Default::default();
                };
                let core_temps = cores::on_each(cores, || {
                    let status = self.io.read_msr(INTEL_CORE_THERM_STATUS).ok()?;
                    intel_core_valid(status).then(|| intel_temp(target, status))
                });
                let values = vec![
                    (Sensor::CpuTemp, intel_temp(target, status)),
                    (
                        Sensor::CpuThrottling,
                        f64::from(u8::from(intel_throttling(status))),
                    ),
                ];
                (values, core_temps)
            }
            Thermal::Amd { tctl_offset, pci } => {
                let values = self
                    .io
                    .read_smn(AMD_SMN_CUR_TEMP, pci)
                    .map(|raw| (Sensor::CpuTemp, amd_temp(raw, *tctl_offset)));
                (values.into_iter().collect(), Vec::new())
            }
        }
    }

    /// `None` on the first sample, and where the counter does not exist.
    fn power(&mut self) -> Option<f64> {
        let now = (self.io.read_msr(self.energy_msr).ok()?, Instant::now());
        let (before, at) = self.energy.replace(now)?;
        watts(before, now.0, self.joules_per_count, now.1 - at).filter(|w| *w > 0.0)
    }

    /// Every sensor that answered; none at all means the driver went away.
    fn sample(&mut self) -> (Vec<(Sensor, f64)>, Vec<f64>) {
        let (mut values, core_temps) = self.thermal();
        values.extend(self.power().map(|v| (Sensor::CpuPower, v)));
        (values, core_temps)
    }
}

/// Publishes the readings every second until `stop` is set.
pub fn run(stop: &AtomicBool) {
    let Ok(publisher) = Publisher::create() else {
        return;
    };
    let mut session: Result<Session, Status> = Err(Status::NoDriver);
    let mut retry_at = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        if session.is_err() && Instant::now() >= retry_at {
            session = Session::connect();
            retry_at = Instant::now() + RETRY;
        }
        match &mut session {
            Ok(s) => {
                let (values, core_temps) = s.sample();
                if values.is_empty() {
                    session = Err(Status::ReadFailed);
                    publisher.publish(Status::ReadFailed, &[], &[]);
                } else {
                    publisher.publish(Status::Ok, &values, &core_temps);
                }
            }
            Err(reason) => publisher.publish(*reason, &[], &[]),
        }
        let wake = Instant::now() + SAMPLE;
        while Instant::now() < wake && !stop.load(Ordering::Relaxed) {
            sleep(STOP_POLL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_machine_has_a_known_vendor_and_a_brand() {
        assert!(vendor().is_some());
        assert!(!brand().is_empty());
        assert_eq!(vendor_of(b"AuthenticAMD"), Some(Vendor::Amd));
        assert_eq!(vendor_of(b"CentaurHauls"), None);
    }

    #[test]
    fn intel_temperature_is_the_distance_below_tj_max() {
        // TjMax 100 °C, 58 °C below it.
        assert_eq!(intel_temp(100 << 16, 0x8000_0000 | 58 << 16), 42.0);
        assert_eq!(intel_temp(90 << 16, 0), 90.0);
    }

    #[test]
    fn intel_status_flags_are_read_from_their_bits() {
        assert!(intel_throttling(0x8839_0001));
        assert!(!intel_throttling(0x8839_0000));
        assert!(intel_core_valid(0x8839_0000));
        assert!(!intel_core_valid(0x0039_0000));
    }

    #[test]
    fn a_missing_tj_max_is_taken_as_100() {
        assert_eq!(intel_temp(0, 30 << 16), 70.0);
    }

    #[test]
    fn amd_temperature_is_in_eighths_of_a_degree() {
        // 52.5 °C is 420 eighths.
        assert_eq!(amd_temp(420 << 21, 0.0), 52.5);
        assert_eq!(amd_temp(420 << 21, 10.0), 42.5);
    }

    #[test]
    fn amd_range_flags_shift_the_scale_down() {
        let raw = 808 << 21;
        assert_eq!(amd_temp(raw, 0.0), 101.0);
        assert_eq!(amd_temp(raw | 1 << 19, 0.0), 52.0);
        assert_eq!(amd_temp(raw | 0b11 << 16, 0.0), 52.0);
        assert_eq!(amd_temp(raw | 0b01 << 16, 0.0), 101.0);
    }

    #[test]
    fn only_some_amd_models_report_a_raised_temperature() {
        assert_eq!(
            amd_tctl_offset("AMD Ryzen 7 2700X Eight-Core Processor"),
            10.0
        );
        assert_eq!(
            amd_tctl_offset("AMD Ryzen Threadripper 2950X 16-Core Processor"),
            27.0
        );
        assert_eq!(amd_tctl_offset("AMD Ryzen 7 5800X3D 8-Core Processor"), 0.0);
        assert_eq!(
            amd_tctl_offset("AMD Ryzen 7 2700 Eight-Core Processor"),
            0.0
        );
    }

    #[test]
    fn the_energy_unit_is_a_power_of_two_fraction() {
        // The usual value: 2^-14 J, about 61 µJ.
        assert_eq!(energy_unit(0x000A_0E03), 1.0 / 16384.0);
    }

    #[test]
    fn power_is_energy_over_time() {
        let unit = 1.0 / 16384.0;
        let second = Duration::from_secs(1);
        assert_eq!(watts(0, 16384 * 65, unit, second), Some(65.0));
        assert_eq!(watts(0, 16384 * 65, unit, second * 2), Some(32.5));
        assert_eq!(watts(0, 1, unit, Duration::ZERO), None);
    }

    #[test]
    fn the_energy_counter_wraps_at_32_bits() {
        let unit = 1.0 / 16384.0;
        let before = 0xFFFF_FFFF - 16383;
        assert_eq!(
            watts(before, 16384, unit, Duration::from_secs(1)),
            Some(2.0)
        );
    }

    #[test]
    fn the_bundled_modules_are_signed() {
        // A signature length, then the signature, then the module.
        for module in [INTEL_MODULE, AMD_MODULE] {
            let signature = u32::from_le_bytes(module[..4].try_into().unwrap()) as usize;
            assert!(signature > 0 && signature + 4 < module.len());
        }
    }
}
