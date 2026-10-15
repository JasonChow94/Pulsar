//! The firmware's description of the machine: board, BIOS, processor socket
//! and memory modules. Windows hands the raw tables to any user.

use windows::Win32::System::SystemInformation::{FIRMWARE_TABLE_PROVIDER, GetSystemFirmwareTable};

/// `RawSMBIOSData` starts with four version bytes and the table length.
const HEADER: usize = 8;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Smbios {
    pub bios: Option<Bios>,
    pub system: Option<Product>,
    pub board: Option<Product>,
    pub processor: Option<Processor>,
    pub memory: MemoryArray,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bios {
    pub vendor: Option<String>,
    pub version: Option<String>,
    pub date: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Product {
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub version: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Processor {
    pub socket: Option<String>,
    /// The reference clock, in MHz.
    pub bus_mhz: Option<u32>,
    pub max_mhz: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryArray {
    pub slots: u32,
    pub max_bytes: Option<u64>,
    /// Every slot, filled or not.
    pub modules: Vec<MemoryModule>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryModule {
    pub slot: Option<String>,
    pub bank: Option<String>,
    /// Zero for an empty slot.
    pub size_bytes: u64,
    pub kind: Option<&'static str>,
    pub form: Option<&'static str>,
    /// Megatransfers per second.
    pub rated_mts: Option<u32>,
    pub configured_mts: Option<u32>,
    pub manufacturer: Option<String>,
    pub part_number: Option<String>,
    pub ranks: Option<u8>,
    pub configured_mv: Option<u16>,
}

impl MemoryModule {
    pub fn installed(&self) -> bool {
        self.size_bytes > 0
    }
}

pub fn read() -> Option<Smbios> {
    let provider = FIRMWARE_TABLE_PROVIDER(u32::from_be_bytes(*b"RSMB"));
    let size = unsafe { GetSystemFirmwareTable(provider, 0, None) };
    if size == 0 {
        return None;
    }
    let mut buffer = vec![0u8; size as usize];
    let written = unsafe { GetSystemFirmwareTable(provider, 0, Some(&mut buffer)) };
    buffer.truncate(written as usize);
    let length = u32::from_le_bytes(buffer.get(4..8)?.try_into().ok()?) as usize;
    let table = buffer.get(HEADER..HEADER + length.min(buffer.len() - HEADER))?;
    Some(parse(table))
}

/// One structure: its formatted area and the strings after it.
struct Structure<'a> {
    kind: u8,
    data: &'a [u8],
    strings: Vec<&'a [u8]>,
}

impl Structure<'_> {
    fn byte(&self, offset: usize) -> Option<u8> {
        self.data.get(offset).copied()
    }

    fn word(&self, offset: usize) -> Option<u16> {
        Some(u16::from_le_bytes(
            self.data.get(offset..offset + 2)?.try_into().ok()?,
        ))
    }

    fn dword(&self, offset: usize) -> Option<u32> {
        Some(u32::from_le_bytes(
            self.data.get(offset..offset + 4)?.try_into().ok()?,
        ))
    }

    fn qword(&self, offset: usize) -> Option<u64> {
        Some(u64::from_le_bytes(
            self.data.get(offset..offset + 8)?.try_into().ok()?,
        ))
    }

    /// The string a byte at `offset` points to; boards fill unset ones with
    /// placeholders, which count as missing.
    fn string(&self, offset: usize) -> Option<String> {
        let index = self.byte(offset)? as usize;
        let raw = self.strings.get(index.checked_sub(1)?)?;
        let s = String::from_utf8_lossy(raw).trim().to_string();
        let placeholder = [
            "",
            "Default string",
            "To Be Filled By O.E.M.",
            "To be filled by O.E.M.",
            "System Product Name",
            "System manufacturer",
            "System Version",
            "Not Specified",
            "Unknown",
            "None",
            "Undefined",
        ]
        .iter()
        .any(|p| s.eq_ignore_ascii_case(p));
        (!placeholder).then_some(s)
    }
}

fn structures(mut table: &[u8]) -> Vec<Structure<'_>> {
    let mut out = Vec::new();
    while table.len() >= 4 {
        let length = table[1] as usize;
        if length < 4 || length > table.len() {
            break;
        }
        let (data, rest) = table.split_at(length);
        // Strings end at a double NUL; a structure without any still has one.
        let Some(end) = rest.windows(2).position(|w| w == [0, 0]) else {
            break;
        };
        let strings = rest[..end]
            .split(|&b| b == 0)
            .filter(|s| !s.is_empty())
            .collect();
        out.push(Structure {
            kind: data[0],
            data,
            strings,
        });
        if data[0] == 127 {
            break;
        }
        table = &rest[end + 2..];
    }
    out
}

fn memory_kind(code: u8) -> Option<&'static str> {
    Some(match code {
        0x12 => "DDR",
        0x13 => "DDR2",
        0x18 => "DDR3",
        0x1A => "DDR4",
        0x1B => "LPDDR",
        0x1C => "LPDDR2",
        0x1D => "LPDDR3",
        0x1E => "LPDDR4",
        0x22 => "DDR5",
        0x23 => "LPDDR5",
        _ => return None,
    })
}

fn memory_form(code: u8) -> Option<&'static str> {
    Some(match code {
        0x09 => "DIMM",
        0x0D => "SODIMM",
        0x0F => "FB-DIMM",
        0x10 => "Die",
        0x11 => "CAMM",
        _ => return None,
    })
}

/// Size of a type 17 structure: a word in MB (or KB with bit 15 set), or
/// `0x7FFF` pointing to a dword in MB.
fn module_size(s: &Structure) -> u64 {
    const MB: u64 = 1024 * 1024;
    match s.word(0x0C) {
        None | Some(0) | Some(0xFFFF) => 0,
        Some(0x7FFF) => s
            .dword(0x1C)
            .map_or(0, |mb| u64::from(mb & 0x7FFF_FFFF) * MB),
        Some(v) if v & 0x8000 != 0 => u64::from(v & 0x7FFF) * 1024,
        Some(v) => u64::from(v) * MB,
    }
}

/// A speed word, where `0xFFFF` defers to a dword further on.
fn speed(s: &Structure, word: usize, extended: usize) -> Option<u32> {
    match s.word(word)? {
        0 => None,
        0xFFFF => s.dword(extended).filter(|&v| v > 0),
        v => Some(u32::from(v)),
    }
}

pub fn parse(table: &[u8]) -> Smbios {
    let mut out = Smbios::default();
    for s in structures(table) {
        match s.kind {
            0 if out.bios.is_none() => {
                out.bios = Some(Bios {
                    vendor: s.string(0x04),
                    version: s.string(0x05),
                    date: s.string(0x08),
                });
            }
            1 if out.system.is_none() => {
                out.system = Some(Product {
                    manufacturer: s.string(0x04),
                    product: s.string(0x05),
                    version: s.string(0x06),
                });
            }
            2 if out.board.is_none() => {
                out.board = Some(Product {
                    manufacturer: s.string(0x04),
                    product: s.string(0x05),
                    version: s.string(0x06),
                });
            }
            4 if out.processor.is_none() => {
                let mhz = |offset| s.word(offset).filter(|&v| v > 0).map(u32::from);
                out.processor = Some(Processor {
                    socket: s.string(0x04),
                    bus_mhz: mhz(0x12),
                    max_mhz: mhz(0x14),
                });
            }
            // System memory arrays only, not caches or flash.
            16 if s.byte(0x05) == Some(0x03) => {
                out.memory.slots += u32::from(s.word(0x0D).unwrap_or(0));
                let max = match s.dword(0x07) {
                    Some(0x8000_0000) => s.qword(0x0F),
                    Some(kb) => Some(u64::from(kb) * 1024),
                    None => None,
                };
                out.memory.max_bytes = match (out.memory.max_bytes, max) {
                    (Some(a), Some(b)) => Some(a + b),
                    (a, b) => a.or(b),
                };
            }
            17 => out.memory.modules.push(MemoryModule {
                slot: s.string(0x10),
                bank: s.string(0x11),
                size_bytes: module_size(&s),
                kind: s.byte(0x12).and_then(memory_kind),
                form: s.byte(0x0E).and_then(memory_form),
                rated_mts: speed(&s, 0x15, 0x54),
                configured_mts: speed(&s, 0x20, 0x58),
                manufacturer: s.string(0x17),
                part_number: s.string(0x1A),
                ranks: s.byte(0x1B).map(|a| a & 0x0F).filter(|&r| r > 0),
                configured_mv: s.word(0x26).filter(|&v| v > 0),
            }),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A structure from its formatted bytes (after the type and length) and strings.
    fn structure(kind: u8, body: &[u8], strings: &[&str]) -> Vec<u8> {
        let mut out = vec![kind, (body.len() + 4) as u8, 0, 0];
        out.extend_from_slice(body);
        for s in strings {
            out.extend_from_slice(s.as_bytes());
            out.push(0);
        }
        if strings.is_empty() {
            out.push(0);
        }
        out.push(0);
        out
    }

    fn module(size: u16, speed: u16, strings: &[&str]) -> Vec<u8> {
        let mut body = vec![0u8; 0x28 - 4];
        let mut put = |offset: usize, bytes: &[u8]| {
            body[offset - 4..offset - 4 + bytes.len()].copy_from_slice(bytes)
        };
        put(0x0C, &size.to_le_bytes());
        put(0x0E, &[0x09]);
        put(0x10, &[1]);
        put(0x11, &[2]);
        put(0x12, &[0x22]);
        put(0x15, &speed.to_le_bytes());
        put(0x17, &[3]);
        put(0x1A, &[4]);
        put(0x1B, &[0x02]);
        put(0x20, &5600u16.to_le_bytes());
        put(0x26, &1250u16.to_le_bytes());
        structure(17, &body, strings)
    }

    #[test]
    fn board_and_bios_strings_are_read() {
        let mut table = structure(
            0,
            &[1, 2, 0, 0, 3],
            &["American Megatrends", "1.40", "05/01/2024"],
        );
        table.extend(structure(
            2,
            &[1, 2, 3],
            &["Micro-Star", "MAG Z790", "Default string"],
        ));
        table.extend(structure(127, &[], &[]));
        let s = parse(&table);
        let bios = s.bios.unwrap();
        assert_eq!(bios.vendor.as_deref(), Some("American Megatrends"));
        assert_eq!(bios.date.as_deref(), Some("05/01/2024"));
        let board = s.board.unwrap();
        assert_eq!(board.product.as_deref(), Some("MAG Z790"));
        assert_eq!(board.version, None, "placeholders count as missing");
    }

    #[test]
    fn memory_modules_and_empty_slots() {
        let mut table = module(16384, 6000, &["DIMM_A1", "BANK 0", "Kingston", "KF560C36"]);
        table.extend(module(0, 0, &["DIMM_A2", "BANK 1"]));
        let s = parse(&table);
        let [filled, empty] = &s.memory.modules[..] else {
            panic!("two slots");
        };
        assert_eq!(filled.size_bytes, 16 << 30);
        assert_eq!(filled.kind, Some("DDR5"));
        assert_eq!(filled.form, Some("DIMM"));
        assert_eq!(filled.rated_mts, Some(6000));
        assert_eq!(filled.configured_mts, Some(5600));
        assert_eq!(filled.part_number.as_deref(), Some("KF560C36"));
        assert_eq!(filled.ranks, Some(2));
        assert_eq!(filled.configured_mv, Some(1250));
        assert!(!empty.installed());
        assert_eq!(empty.slot.as_deref(), Some("DIMM_A2"));
    }

    #[test]
    fn large_modules_use_the_extended_size() {
        let mut bytes = module(0x7FFF, 6000, &[]);
        bytes[0x1C..0x20].copy_from_slice(&(48u32 * 1024).to_le_bytes());
        let s = parse(&bytes);
        assert_eq!(s.memory.modules[0].size_bytes, 48 << 30);
    }

    #[test]
    fn a_truncated_table_stops_cleanly() {
        let mut table = structure(0, &[1, 2, 0, 0, 3], &["Vendor"]);
        table.truncate(table.len() - 3);
        assert_eq!(parse(&table), Smbios::default());
        assert_eq!(parse(&[]), Smbios::default());
    }
}
