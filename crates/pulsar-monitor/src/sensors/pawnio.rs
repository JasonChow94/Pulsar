//! A client for the PawnIO driver, which runs signed modules that expose a
//! fixed set of hardware registers. Only its device control interface is used.

use windows::Win32::Foundation::{
    CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, HLOCAL, LocalFree, WAIT_ABANDONED,
    WAIT_OBJECT_0,
};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};
use windows::core::{Result, w};

const DEVICE_TYPE: u32 = 41394;
const IOCTL_LOAD_BINARY: u32 = control_code(0x821);
const IOCTL_EXECUTE_FN: u32 = control_code(0x841);
/// Function names are sent as a fixed, zero-padded field.
const NAME_LEN: usize = 32;
/// How long to wait for another monitoring tool to finish with the PCI bus.
const PCI_WAIT_MS: u32 = 100;

/// The mutex hardware monitors share so their two-step PCI register reads
/// do not interleave. Anyone may take it, as the other tools expect.
pub struct PciLock(HANDLE);

impl PciLock {
    pub fn open() -> Result<Self> {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                w!("D:(A;;GA;;;WD)"),
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
        let mutex = unsafe { CreateMutexW(Some(&attributes), false, w!(r"Global\Access_PCI")) };
        unsafe {
            let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        }
        mutex.map(Self)
    }

    /// Runs `f` holding the lock; `None` when another tool kept it too long.
    fn with<R>(&self, f: impl FnOnce() -> R) -> Option<R> {
        let wait = unsafe { WaitForSingleObject(self.0, PCI_WAIT_MS) };
        if wait != WAIT_OBJECT_0 && wait != WAIT_ABANDONED {
            return None;
        }
        let result = f();
        unsafe {
            let _ = ReleaseMutex(self.0);
        }
        Some(result)
    }
}

impl Drop for PciLock {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// A buffered control code that needs no particular file access.
const fn control_code(function: u32) -> u32 {
    DEVICE_TYPE << 16 | function << 2
}

/// The function name field followed by the 64-bit arguments.
fn request(function: &str, input: &[u64]) -> Vec<u8> {
    let mut bytes = vec![0u8; NAME_LEN];
    let name = &function.as_bytes()[..function.len().min(NAME_LEN - 1)];
    bytes[..name.len()].copy_from_slice(name);
    bytes.extend(input.iter().flat_map(|v| v.to_le_bytes()));
    bytes
}

/// One open handle holds one loaded module.
pub struct PawnIo(HANDLE);

impl PawnIo {
    /// Fails when the driver is not installed or the caller is not elevated.
    pub fn open() -> Result<Self> {
        unsafe {
            CreateFileW(
                w!(r"\\?\GLOBALROOT\Device\PawnIO"),
                (GENERIC_READ | GENERIC_WRITE).0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            )
        }
        .map(Self)
    }

    /// The driver rejects a module that is not signed by its author, and the
    /// module rejects hardware it does not support.
    pub fn load(&self, module: &[u8]) -> Result<()> {
        let mut returned = 0;
        unsafe {
            DeviceIoControl(
                self.0,
                IOCTL_LOAD_BINARY,
                Some(module.as_ptr().cast()),
                module.len() as u32,
                None,
                0,
                Some(&mut returned),
                None,
            )
        }
    }

    fn execute(&self, function: &str, input: &[u64], output: &mut [u64]) -> Result<()> {
        let request = request(function, input);
        let mut returned = 0;
        unsafe {
            DeviceIoControl(
                self.0,
                IOCTL_EXECUTE_FN,
                Some(request.as_ptr().cast()),
                request.len() as u32,
                Some(output.as_mut_ptr().cast()),
                size_of_val(output) as u32,
                Some(&mut returned),
                None,
            )
        }
    }

    /// Reads a register the loaded MSR module allows.
    pub fn read_msr(&self, msr: u32) -> Result<u64> {
        let mut value = [0u64];
        self.execute("ioctl_read_msr", &[u64::from(msr)], &mut value)?;
        Ok(value[0])
    }
}

impl PawnIo {
    /// Reads an AMD system management network register through the AMD module.
    pub fn read_smn(&self, offset: u32, pci: &PciLock) -> Option<u32> {
        let mut value = [0u64];
        pci.with(|| self.execute("ioctl_read_smn", &[u64::from(offset)], &mut value))?
            .ok()?;
        Some(value[0] as u32)
    }
}

impl Drop for PawnIo {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_codes_match_the_driver() {
        assert_eq!(IOCTL_LOAD_BINARY, 0xA1B2_2084);
        assert_eq!(IOCTL_EXECUTE_FN, 0xA1B2_2104);
    }

    #[test]
    fn a_request_is_a_padded_name_then_the_arguments() {
        let bytes = request("ioctl_read_msr", &[0x1B1]);
        assert_eq!(bytes.len(), NAME_LEN + 8);
        assert_eq!(&bytes[..14], b"ioctl_read_msr");
        assert!(bytes[14..NAME_LEN].iter().all(|&b| b == 0));
        assert_eq!(bytes[NAME_LEN..], 0x1B1u64.to_le_bytes());
    }

    #[test]
    fn the_pci_lock_can_be_taken_again_after_release() {
        let lock = PciLock::open().unwrap();
        assert_eq!(lock.with(|| 1), Some(1));
        assert_eq!(lock.with(|| 2), Some(2));
    }

    #[test]
    fn an_overlong_name_keeps_its_terminator() {
        let bytes = request(&"x".repeat(40), &[]);
        assert_eq!(bytes.len(), NAME_LEN);
        assert_eq!(bytes[NAME_LEN - 1], 0);
    }
}
