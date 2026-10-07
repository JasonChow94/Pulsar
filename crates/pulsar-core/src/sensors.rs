//! The shared memory through which the elevated sensor service hands
//! readings to the unelevated app. It carries values one way only: the app
//! never sends the service anything.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, INVALID_HANDLE_VALUE, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP_READ, FILE_MAP_WRITE, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile,
    OpenFileMappingW, PAGE_READWRITE, UnmapViewOfFile,
};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::core::{HSTRING, w};

pub const SERVICE_NAME: &str = "PulsarSensors";
const MAPPING_NAME: &str = r"Global\Pulsar.Sensors";
const MAGIC: u32 = 0x534E_5350;
const VERSION: u32 = 1;
/// A reading older than this means the service has stopped.
const STALE_MS: u64 = 5000;
/// The service and administrators own the section; everyone else reads it.
const SECURITY: windows::core::PCWSTR = w!("D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GR;;;AU)");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reading {
    /// Tenths of a degree Celsius.
    CpuTemp(i32),
    NoDriver,
    UnsupportedCpu,
    ReadFailed,
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
    #[error("The CPU temperature could not be read")]
    ReadFailed,
}

#[repr(C)]
struct Shared {
    magic: AtomicU32,
    version: AtomicU32,
    /// Kind in the high half, value in the low half, so a reader never sees
    /// one without the other.
    state: AtomicU64,
    /// `GetTickCount64` at the last update.
    tick_ms: AtomicU64,
}

fn encode(reading: Reading) -> u64 {
    let (kind, value) = match reading {
        Reading::CpuTemp(deci_c) => (0u64, deci_c as u32),
        Reading::NoDriver => (1, 0),
        Reading::UnsupportedCpu => (2, 0),
        Reading::ReadFailed => (3, 0),
    };
    kind << 32 | u64::from(value)
}

fn decode(state: u64) -> Reading {
    match state >> 32 {
        0 => Reading::CpuTemp(state as u32 as i32),
        1 => Reading::NoDriver,
        2 => Reading::UnsupportedCpu,
        _ => Reading::ReadFailed,
    }
}

/// What a reader makes of the section's contents at `now_ms`.
fn interpret(
    magic: u32,
    version: u32,
    state: u64,
    tick_ms: u64,
    now_ms: u64,
) -> Result<f64, Unavailable> {
    if magic != MAGIC || version != VERSION {
        return Err(Unavailable::WrongVersion);
    }
    if now_ms.saturating_sub(tick_ms) > STALE_MS {
        return Err(Unavailable::NotRunning);
    }
    match decode(state) {
        Reading::CpuTemp(deci_c) => Ok(f64::from(deci_c) / 10.0),
        Reading::NoDriver => Err(Unavailable::NoDriver),
        Reading::UnsupportedCpu => Err(Unavailable::UnsupportedCpu),
        Reading::ReadFailed => Err(Unavailable::ReadFailed),
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

fn map(handle: HANDLE, access: windows::Win32::System::Memory::FILE_MAP) -> Option<View> {
    let address = unsafe { MapViewOfFile(handle, access, 0, 0, size_of::<Shared>()) };
    if address.Value.is_null() {
        unsafe {
            let _ = CloseHandle(handle);
        }
        return None;
    }
    Some(View { handle, address })
}

/// The CPU temperature in °C. Opens the section on every call, so a
/// restarted service is picked up.
pub fn cpu_temp() -> Result<f64, Unavailable> {
    let name = HSTRING::from(MAPPING_NAME);
    let handle = unsafe { OpenFileMappingW(FILE_MAP_READ.0, false, &name) }
        .map_err(|_| Unavailable::NotInstalled)?;
    let view = map(handle, FILE_MAP_READ).ok_or(Unavailable::NotInstalled)?;
    let s = view.shared();
    interpret(
        s.magic.load(Ordering::Relaxed),
        s.version.load(Ordering::Relaxed),
        s.state.load(Ordering::Relaxed),
        s.tick_ms.load(Ordering::Relaxed),
        unsafe { GetTickCount64() },
    )
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

    pub fn publish(&self, reading: Reading) {
        let s = self.0.shared();
        s.state.store(encode(reading), Ordering::Relaxed);
        s.tick_ms
            .store(unsafe { GetTickCount64() }, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readings_survive_the_shared_word() {
        for reading in [
            Reading::CpuTemp(415),
            Reading::CpuTemp(-50),
            Reading::CpuTemp(0),
            Reading::NoDriver,
            Reading::UnsupportedCpu,
            Reading::ReadFailed,
        ] {
            assert_eq!(decode(encode(reading)), reading);
        }
    }

    #[test]
    fn a_fresh_temperature_is_reported_in_degrees() {
        let state = encode(Reading::CpuTemp(415));
        assert_eq!(interpret(MAGIC, VERSION, state, 1000, 1500), Ok(41.5));
    }

    #[test]
    fn a_stale_reading_means_the_service_stopped() {
        let state = encode(Reading::CpuTemp(415));
        assert_eq!(
            interpret(MAGIC, VERSION, state, 1000, 1000 + STALE_MS + 1),
            Err(Unavailable::NotRunning)
        );
    }

    #[test]
    fn another_layout_is_never_read_as_a_temperature() {
        let state = encode(Reading::CpuTemp(415));
        assert_eq!(
            interpret(MAGIC, VERSION + 1, state, 0, 0),
            Err(Unavailable::WrongVersion)
        );
        assert_eq!(
            interpret(0, VERSION, state, 0, 0),
            Err(Unavailable::WrongVersion)
        );
    }

    #[test]
    fn the_service_says_why_it_has_no_reading() {
        let failed = |r| interpret(MAGIC, VERSION, encode(r), 0, 0);
        assert_eq!(failed(Reading::NoDriver), Err(Unavailable::NoDriver));
        assert_eq!(
            failed(Reading::UnsupportedCpu),
            Err(Unavailable::UnsupportedCpu)
        );
        assert_eq!(failed(Reading::ReadFailed), Err(Unavailable::ReadFailed));
        assert_eq!(Unavailable::NoDriver.to_string(), "PawnIO is not installed");
    }
}
