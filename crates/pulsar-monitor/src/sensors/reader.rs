use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};

use pulsar_core::sensors::{Publisher, Sensor, Status};
use windows::Win32::Foundation::ERROR_NOT_SUPPORTED;

use crate::pawnio::PawnIo;

const MODULE: &[u8] = include_bytes!("../../resources/pawnio/IntelMSR.bin");
const MSR_TEMPERATURE_TARGET: u32 = 0x1A2;
const MSR_PACKAGE_THERM_STATUS: u32 = 0x1B1;
const MSR_RAPL_POWER_UNIT: u32 = 0x606;
const MSR_PKG_ENERGY_STATUS: u32 = 0x611;
const SAMPLE: Duration = Duration::from_secs(1);
/// How often to look for a driver that was missing or went away.
const RETRY: Duration = Duration::from_secs(10);
const STOP_POLL: Duration = Duration::from_millis(100);

/// Package temperature in °C: the CPU reports how far it is below its
/// throttling point, which the target register names.
fn package_temp(target: u64, status: u64) -> f64 {
    let tj_max = match (target >> 16) & 0xFF {
        0 => 100,
        t => t as i32,
    };
    let below = ((status >> 16) & 0x7F) as i32;
    f64::from(tj_max - below)
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

/// A loaded module and what the last sample left behind.
struct Session {
    io: PawnIo,
    /// Zero where the platform has no energy counters.
    joules_per_count: f64,
    energy: Option<(u64, Instant)>,
}

impl Session {
    fn connect() -> Result<Self, Status> {
        let io = PawnIo::open().map_err(|_| Status::NoDriver)?;
        io.load(MODULE).map_err(|e| {
            if e.code() == ERROR_NOT_SUPPORTED.to_hresult() {
                Status::UnsupportedCpu
            } else {
                Status::ReadFailed
            }
        })?;
        let joules_per_count = io.read_msr(MSR_RAPL_POWER_UNIT).map_or(0.0, energy_unit);
        Ok(Self {
            io,
            joules_per_count,
            energy: None,
        })
    }

    fn temp(&self) -> Option<f64> {
        let target = self.io.read_msr(MSR_TEMPERATURE_TARGET).ok()?;
        let status = self.io.read_msr(MSR_PACKAGE_THERM_STATUS).ok()?;
        Some(package_temp(target, status))
    }

    /// `None` on the first sample, and where the counter does not exist.
    fn power(&mut self) -> Option<f64> {
        let now = (
            self.io.read_msr(MSR_PKG_ENERGY_STATUS).ok()?,
            Instant::now(),
        );
        let (before, at) = self.energy.replace(now)?;
        watts(before, now.0, self.joules_per_count, now.1 - at).filter(|w| *w > 0.0)
    }

    /// Every sensor that answered; none at all means the driver went away.
    fn sample(&mut self) -> Vec<(Sensor, f64)> {
        let mut values = Vec::new();
        values.extend(self.temp().map(|v| (Sensor::CpuTemp, v)));
        values.extend(self.power().map(|v| (Sensor::CpuPower, v)));
        values
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
                let values = s.sample();
                if values.is_empty() {
                    session = Err(Status::ReadFailed);
                    publisher.publish(Status::ReadFailed, &[]);
                } else {
                    publisher.publish(Status::Ok, &values);
                }
            }
            Err(reason) => publisher.publish(*reason, &[]),
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
    fn temperature_is_the_distance_below_tj_max() {
        // TjMax 100 °C, 58 °C below it.
        assert_eq!(package_temp(100 << 16, 0x8000_0000 | 58 << 16), 42.0);
        assert_eq!(package_temp(90 << 16, 0), 90.0);
    }

    #[test]
    fn a_missing_tj_max_is_taken_as_100() {
        assert_eq!(package_temp(0, 30 << 16), 70.0);
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
    fn the_bundled_module_is_signed() {
        // A signature length, then the signature, then the module.
        let signature = u32::from_le_bytes(MODULE[..4].try_into().unwrap()) as usize;
        assert!(signature > 0 && signature + 4 < MODULE.len());
    }
}
