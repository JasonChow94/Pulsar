//! The shared memory through which the elevated sensor service hands
//! readings to the unelevated app. It carries values one way only: the app
//! never sends the service anything.

use std::sync::atomic::{AtomicI64, AtomicU32, AtomicU64, Ordering};

use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, INVALID_HANDLE_VALUE, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP, FILE_MAP_READ, FILE_MAP_WRITE, MEMORY_MAPPED_VIEW_ADDRESS,
    MapViewOfFile, OpenFileMappingW, PAGE_READWRITE, UnmapViewOfFile,
};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::core::{HSTRING, PCWSTR, w};

pub const SERVICE_NAME: &str = "PulsarSensors";
const MAPPING_NAME: &str = r"Global\Pulsar.Sensors";
const MAGIC: u32 = 0x534E_5350;
const VERSION: u32 = 3;
/// A reading older than this means the service has stopped.
const STALE_MS: u64 = 5000;
/// The service and administrators own the section; everyone else reads it.
const SECURITY: PCWSTR = w!("D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GR;;;AU)");
/// More slots than sensors, so adding one keeps the layout.
const SENSOR_SLOTS: usize = 16;
/// Per-core temperatures follow the sensors, one slot a core.
const CORE_SLOTS: usize = 256;
const SLOTS: usize = SENSOR_SLOTS + CORE_SLOTS;
const MISSING: i64 = i64::MIN;

/// A slot in the section. Values are thousandths of the sensor's unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sensor {
    /// °C.
    CpuTemp = 0,
    /// Watts.
    CpuPower = 1,
    /// 1 while the CPU is slowing itself down to cool off, else 0.
    CpuThrottling = 2,
    /// °C.
    GpuHotSpot = 3,
    /// °C, the hottest memory chip.
    GpuMemoryTemp = 4,
}

/// Why the service has nothing to report, when it is running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ok = 0,
    NoDriver = 1,
    UnsupportedCpu = 2,
    ReadFailed = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Unavailable {
    #[error("The Pulsar sensor service is not installed")]
    NotInstalled,
    #[error("The Pulsar sensor service is not running")]
    NotRunning,
    #[error("The Pulsar sensor service needs updating")]
    WrongVersion,
    #[error("PawnIO is not installed")]
    NoDriver,
    #[error("This CPU is not supported yet")]
    UnsupportedCpu,
    #[error("The CPU sensors could not be read")]
    ReadFailed,
}

#[repr(C)]
struct Shared {
    magic: AtomicU32,
    version: AtomicU32,
    status: AtomicU32,
    /// `GetTickCount64` at the last update.
    tick_ms: AtomicU64,
    values: [AtomicI64; SLOTS],
}

/// One sample of every sensor the service could read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Readings([i64; SLOTS]);

impl Readings {
    pub fn get(&self, sensor: Sensor) -> Option<f64> {
        let raw = self.0[sensor as usize];
        (raw != MISSING).then(|| raw as f64 / 1000.0)
    }

    /// °C for each physical core, in processor order; empty where the CPU
    /// has no per-core sensors.
    pub fn core_temps(&self) -> Vec<f64> {
        self.0[SENSOR_SLOTS..]
            .iter()
            .take_while(|&&raw| raw != MISSING)
            .map(|&raw| raw as f64 / 1000.0)
            .collect()
    }
}

fn milli(value: f64) -> i64 {
    if value.is_finite() {
        (value * 1000.0).round() as i64
    } else {
        MISSING
    }
}

fn slots(values: &[(Sensor, f64)], core_temps: &[f64]) -> [i64; SLOTS] {
    let mut slots = [MISSING; SLOTS];
    for &(sensor, value) in values {
        slots[sensor as usize] = milli(value);
    }
    for (slot, &temp) in slots[SENSOR_SLOTS..].iter_mut().zip(core_temps) {
        *slot = milli(temp);
    }
    slots
}

/// What a reader makes of the section's header at `now_ms`.
fn check(
    magic: u32,
    version: u32,
    status: u32,
    tick_ms: u64,
    now_ms: u64,
) -> Result<(), Unavailable> {
    if magic != MAGIC || version != VERSION {
        return Err(Unavailable::WrongVersion);
    }
    if now_ms.saturating_sub(tick_ms) > STALE_MS {
        return Err(Unavailable::NotRunning);
    }
    match status {
        0 => Ok(()),
        1 => Err(Unavailable::NoDriver),
        2 => Err(Unavailable::UnsupportedCpu),
        _ => Err(Unavailable::ReadFailed),
    }
}

struct View {
    handle: HANDLE,
    address: MEMORY_MAPPED_VIEW_ADDRESS,
}

impl View {
    fn shared(&self) -> &Shared {
        // The section is at least `size_of::<Shared>()` bytes and page-aligned.
        unsafe { &*self.address.Value.cast::<Shared>() }
    }
}

impl Drop for View {
    fn drop(&mut self) {
        unsafe {
            let _ = UnmapViewOfFile(self.address);
            let _ = CloseHandle(self.handle);
        }
    }
}

fn map(handle: HANDLE, access: FILE_MAP) -> Option<View> {
    let address = unsafe { MapViewOfFile(handle, access, 0, 0, size_of::<Shared>()) };
    if address.Value.is_null() {
        unsafe {
            let _ = CloseHandle(handle);
        }
        return None;
    }
    Some(View { handle, address })
}

/// The service's latest readings. Opens the section on every call, so a
/// restarted service is picked up.
pub fn read() -> Result<Readings, Unavailable> {
    let name = HSTRING::from(MAPPING_NAME);
    let handle = unsafe { OpenFileMappingW(FILE_MAP_READ.0, false, &name) }
        .map_err(|_| Unavailable::NotInstalled)?;
    let view = map(handle, FILE_MAP_READ).ok_or(Unavailable::NotInstalled)?;
    let s = view.shared();
    check(
        s.magic.load(Ordering::Relaxed),
        s.version.load(Ordering::Relaxed),
        s.status.load(Ordering::Relaxed),
        s.tick_ms.load(Ordering::Relaxed),
        unsafe { GetTickCount64() },
    )?;
    Ok(Readings(std::array::from_fn(|i| {
        s.values[i].load(Ordering::Relaxed)
    })))
}

/// The service's side of the section.
pub struct Publisher(View);

// The view is only touched through atomics.
unsafe impl Send for Publisher {}

impl Publisher {
    pub fn create() -> windows::core::Result<Self> {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                SECURITY,
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )?;
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: false.into(),
        };
        let handle = unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                Some(&attributes),
                PAGE_READWRITE,
                0,
                size_of::<Shared>() as u32,
                &HSTRING::from(MAPPING_NAME),
            )
        };
        unsafe {
            let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        }
        let view = map(handle?, FILE_MAP_WRITE).ok_or_else(windows::core::Error::from_thread)?;
        let s = view.shared();
        s.version.store(VERSION, Ordering::Relaxed);
        s.magic.store(MAGIC, Ordering::Relaxed);
        Ok(Self(view))
    }

    /// Sensors left out of `values` read as missing.
    pub fn publish(&self, status: Status, values: &[(Sensor, f64)], core_temps: &[f64]) {
        let s = self.0.shared();
        for (slot, value) in s.values.iter().zip(slots(values, core_temps)) {
            slot.store(value, Ordering::Relaxed);
        }
        s.status.store(status as u32, Ordering::Relaxed);
        s.tick_ms
            .store(unsafe { GetTickCount64() }, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_values_read_back_and_the_rest_are_missing() {
        let r = Readings(slots(&[(Sensor::CpuTemp, 41.5)], &[]));
        assert_eq!(r.get(Sensor::CpuTemp), Some(41.5));
        assert_eq!(r.get(Sensor::CpuPower), None);
        let r = Readings(slots(
            &[(Sensor::CpuPower, 87.25), (Sensor::CpuTemp, -5.0)],
            &[],
        ));
        assert_eq!(r.get(Sensor::CpuPower), Some(87.25));
        assert_eq!(r.get(Sensor::CpuTemp), Some(-5.0));
    }

    #[test]
    fn a_value_that_is_not_a_number_is_missing() {
        let r = Readings(slots(&[(Sensor::CpuPower, f64::NAN)], &[]));
        assert_eq!(r.get(Sensor::CpuPower), None);
    }

    #[test]
    fn core_temperatures_keep_their_order_and_count() {
        let r = Readings(slots(&[(Sensor::CpuThrottling, 1.0)], &[40.0, 55.5, 38.0]));
        assert_eq!(r.core_temps(), [40.0, 55.5, 38.0]);
        assert_eq!(r.get(Sensor::CpuThrottling), Some(1.0));
        assert!(Readings(slots(&[], &[])).core_temps().is_empty());
        let many = vec![50.0; CORE_SLOTS + 10];
        assert_eq!(Readings(slots(&[], &many)).core_temps().len(), CORE_SLOTS);
    }

    #[test]
    fn a_fresh_section_is_usable() {
        assert_eq!(check(MAGIC, VERSION, 0, 1000, 1500), Ok(()));
    }

    #[test]
    fn a_stale_section_means_the_service_stopped() {
        assert_eq!(
            check(MAGIC, VERSION, 0, 1000, 1000 + STALE_MS + 1),
            Err(Unavailable::NotRunning)
        );
    }

    #[test]
    fn another_layout_is_never_read() {
        assert_eq!(
            check(MAGIC, VERSION + 1, 0, 0, 0),
            Err(Unavailable::WrongVersion)
        );
        assert_eq!(check(0, VERSION, 0, 0, 0), Err(Unavailable::WrongVersion));
    }

    #[test]
    fn the_service_says_why_it_has_no_readings() {
        let failed = |status: Status| check(MAGIC, VERSION, status as u32, 0, 0);
        assert_eq!(failed(Status::NoDriver), Err(Unavailable::NoDriver));
        assert_eq!(
            failed(Status::UnsupportedCpu),
            Err(Unavailable::UnsupportedCpu)
        );
        assert_eq!(failed(Status::ReadFailed), Err(Unavailable::ReadFailed));
        assert_eq!(Unavailable::NoDriver.to_string(), "PawnIO is not installed");
    }
}
