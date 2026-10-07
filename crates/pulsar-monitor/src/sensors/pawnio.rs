//! A client for the PawnIO driver, which runs signed modules that expose a
//! fixed set of hardware registers. Only its device control interface is used.

use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::core::{Result, w};

const DEVICE_TYPE: u32 = 41394;
const IOCTL_LOAD_BINARY: u32 = control_code(0x821);
const IOCTL_EXECUTE_FN: u32 = control_code(0x841);
/// Function names are sent as a fixed, zero-padded field.
const NAME_LEN: usize = 32;

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
    fn an_overlong_name_keeps_its_terminator() {
        let bytes = request(&"x".repeat(40), &[]);
        assert_eq!(bytes.len(), NAME_LEN);
        assert_eq!(bytes[NAME_LEN - 1], 0);
    }
}
