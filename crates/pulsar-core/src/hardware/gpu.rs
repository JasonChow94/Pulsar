//! Graphics adapters as DirectX lists them.

use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIDevice, IDXGIFactory1,
};
use windows::core::Interface;

const MICROSOFT: u32 = 0x1414;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GpuInfo {
    pub name: String,
    pub vendor: Option<&'static str>,
    pub vendor_id: u32,
    pub device_id: u32,
    pub dedicated_bytes: u64,
    pub shared_bytes: u64,
    pub driver: Option<String>,
}

impl GpuInfo {
    /// Integrated parts carve their memory from system RAM.
    pub fn integrated(&self) -> bool {
        self.dedicated_bytes < 512 * 1024 * 1024
    }
}

pub fn vendor_name(id: u32) -> Option<&'static str> {
    Some(match id {
        0x10DE => "NVIDIA",
        0x1002 | 0x1022 => "AMD",
        0x8086 => "Intel",
        0x5143 => "Qualcomm",
        _ => return None,
    })
}

/// The driver's file version, `32.0.15.9144`; NVIDIA's own numbering is
/// the last five digits, `591.44`.
pub fn driver_version(raw: i64, vendor_id: u32) -> String {
    let raw = raw as u64;
    let parts = [
        raw >> 48,
        (raw >> 32) & 0xFFFF,
        (raw >> 16) & 0xFFFF,
        raw & 0xFFFF,
    ];
    if vendor_id == 0x10DE {
        let digits = (parts[2] % 10) * 10_000 + parts[3];
        return format!("{}.{:02}", digits / 100, digits % 100);
    }
    parts.map(|p| p.to_string()).join(".")
}

pub fn read() -> Vec<GpuInfo> {
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
        return Vec::new();
    };
    let mut out: Vec<GpuInfo> = Vec::new();
    for i in 0.. {
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(i) }) else {
            break;
        };
        let Ok(desc) = (unsafe { adapter.GetDesc1() }) else {
            continue;
        };
        if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 || desc.VendorId == MICROSOFT {
            continue;
        }
        let name_len = desc.Description.iter().position(|&c| c == 0).unwrap_or(128);
        let driver = unsafe { adapter.CheckInterfaceSupport(&IDXGIDevice::IID) }
            .ok()
            .map(|v| driver_version(v, desc.VendorId));
        let gpu = GpuInfo {
            name: super::cpu::clean_brand(&String::from_utf16_lossy(&desc.Description[..name_len])),
            vendor: vendor_name(desc.VendorId),
            vendor_id: desc.VendorId,
            device_id: desc.DeviceId,
            dedicated_bytes: desc.DedicatedVideoMemory as u64,
            shared_bytes: desc.SharedSystemMemory as u64,
            driver,
        };
        // An adapter shows once per output-capable node on some systems.
        if !out
            .iter()
            .any(|g| g.device_id == gpu.device_id && g.name == gpu.name)
        {
            out.push(gpu);
        }
    }
    // The discrete card first: it is the one the sensors describe.
    out.sort_by_key(|g| std::cmp::Reverse(g.dedicated_bytes));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvidia_drivers_read_as_nvidia_numbers_them() {
        let raw = (32i64 << 48) | (15 << 16) | 9144;
        assert_eq!(driver_version(raw, 0x10DE), "591.44");
        assert_eq!(driver_version(raw, 0x1002), "32.0.15.9144");
    }

    #[test]
    fn vendors_are_named_by_pci_id() {
        assert_eq!(vendor_name(0x10DE), Some("NVIDIA"));
        assert_eq!(vendor_name(0x1234), None);
    }
}
