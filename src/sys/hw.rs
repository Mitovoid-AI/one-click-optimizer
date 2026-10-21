//! Hardware and system facts for the health checks: installed RAM modules
//! (read from the SMBIOS firmware table, no WMI), uptime and free disk space.

use crate::util::wide;
use serde::Serialize;
use std::ffi::c_void;

#[link(name = "kernel32")]
extern "system" {
    fn GetSystemFirmwareTable(provider: u32, id: u32, buf: *mut c_void, size: u32) -> u32;
    fn GetTickCount64() -> u64;
    fn GetDiskFreeSpaceExW(dir: *const u16, avail: *mut u64, total: *mut u64, free: *mut u64) -> i32;
}

/// 'RSMB': the raw SMBIOS table provider.
const RSMB: u32 = u32::from_be_bytes(*b"RSMB");

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
pub struct RamModule {
    pub locator: String,
    pub size_mb: u64,
    pub speed_mts: u32,
    pub kind: String,
    pub form: String,
    /// Built into the motherboard (cannot be replaced).
    pub soldered: bool,
    pub part: String,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
pub struct RamInfo {
    pub modules: Vec<RamModule>,
    pub empty_slots: usize,
    /// What the memory controller supports (often more than the laptop maker lists).
    pub max_capacity_mb: u64,
}

impl RamInfo {
    pub fn total_mb(&self) -> u64 {
        self.modules.iter().map(|m| m.size_mb).sum()
    }
    pub fn soldered_mb(&self) -> u64 {
        self.modules.iter().filter(|m| m.soldered).map(|m| m.size_mb).sum()
    }
    pub fn removable(&self) -> Vec<&RamModule> {
        self.modules.iter().filter(|m| !m.soldered).collect()
    }
}

fn u16_at(s: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(s.get(o..o + 2)?.try_into().ok()?))
}
fn u32_at(s: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(s.get(o..o + 4)?.try_into().ok()?))
}
fn u64_at(s: &[u8], o: usize) -> Option<u64> {
    Some(u64::from_le_bytes(s.get(o..o + 8)?.try_into().ok()?))
}

fn form_name(f: u8) -> &'static str {
    match f {
        0x09 => "DIMM",
        0x0B => "Built-in chips",
        0x0D => "SO-DIMM (laptop stick)",
        0x0F => "FB-DIMM",
        _ => "Other",
    }
}

fn type_name(t: u8) -> &'static str {
    match t {
        0x18 => "DDR3",
        0x1A => "DDR4",
        0x1B => "LPDDR",
        0x1C => "LPDDR2",
        0x1D => "LPDDR3",
        0x1E => "LPDDR4",
        0x22 => "DDR5",
        0x23 => "LPDDR5",
        _ => "RAM",
    }
}

/// Parses the SMBIOS structure table (type 16 = memory array, 17 = memory device).
pub fn parse_smbios(table: &[u8]) -> RamInfo {
    let mut info = RamInfo::default();
    let mut off = 0usize;
    while off + 4 <= table.len() {
        let ty = table[off];
        let len = table[off + 1] as usize;
        if len < 4 || off + len > table.len() {
            break;
        }
        let s = &table[off..off + len];
        // String set: NUL-separated, ends with a double NUL.
        let mut pos = off + len;
        let mut strings: Vec<String> = Vec::new();
        if table.get(pos) == Some(&0) && table.get(pos + 1) == Some(&0) {
            pos += 2;
        } else {
            loop {
                let start = pos;
                while pos < table.len() && table[pos] != 0 {
                    pos += 1;
                }
                if pos >= table.len() {
                    return info;
                }
                strings.push(String::from_utf8_lossy(&table[start..pos]).trim().to_string());
                pos += 1;
                if pos >= table.len() || table[pos] == 0 {
                    pos += 1;
                    break;
                }
            }
        }
        let get = |idx: Option<&u8>| -> String {
            match idx {
                Some(&i) if i > 0 => strings.get(i as usize - 1).cloned().unwrap_or_default(),
                _ => String::new(),
            }
        };
        match ty {
            16 if s.get(5) == Some(&3) => {
                // System memory array.
                let max_kb = u32_at(s, 7).unwrap_or(0);
                info.max_capacity_mb += if max_kb == 0x8000_0000 {
                    u64_at(s, 0x0F).unwrap_or(0) >> 20
                } else {
                    max_kb as u64 / 1024
                };
            }
            17 if len >= 0x15 => {
                let size = u16_at(s, 0x0C).unwrap_or(0);
                let mb = match size {
                    0 | 0xFFFF => 0,
                    0x7FFF => u32_at(s, 0x1C).unwrap_or(0) as u64 & 0x7FFF_FFFF,
                    v if v & 0x8000 != 0 => (v & 0x7FFF) as u64 / 1024,
                    v => v as u64,
                };
                if mb == 0 {
                    info.empty_slots += 1;
                } else {
                    let form = s[0x0E];
                    let locator = get(s.get(0x10));
                    let l = locator.to_ascii_lowercase();
                    info.modules.push(RamModule {
                        soldered: form == 0x0B || !(l.contains("dimm") || l.contains("slot")),
                        locator,
                        size_mb: mb,
                        speed_mts: u16_at(s, 0x15).unwrap_or(0) as u32,
                        kind: type_name(s[0x12]).into(),
                        form: form_name(form).into(),
                        part: get(s.get(0x1A)),
                    });
                }
            }
            127 => break,
            _ => {}
        }
        off = pos;
    }
    info
}

pub fn read_ram() -> Option<RamInfo> {
    unsafe {
        let size = GetSystemFirmwareTable(RSMB, 0, std::ptr::null_mut(), 0);
        if size < 8 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        if GetSystemFirmwareTable(RSMB, 0, buf.as_mut_ptr() as *mut c_void, size) != size {
            return None;
        }
        // RawSMBIOSData header: 4 version bytes + u32 length, then the table.
        let len = u32_at(&buf, 4)? as usize;
        let table = buf.get(8..8 + len.min(buf.len() - 8))?;
        let info = parse_smbios(table);
        (!info.modules.is_empty()).then_some(info)
    }
}

pub fn uptime_secs() -> u64 {
    unsafe { GetTickCount64() / 1000 }
}

/// (free, total) bytes of the drive holding `path`.
pub fn disk_space(path: &str) -> Option<(u64, u64)> {
    let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
    let ok = unsafe { GetDiskFreeSpaceExW(wide(path).as_ptr(), &mut avail, &mut total, &mut free) };
    (ok != 0).then_some((avail, total))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds one SMBIOS structure with its string set.
    fn structure(ty: u8, body: &[u8], strings: &[&str]) -> Vec<u8> {
        let mut s = vec![ty, (body.len() + 4) as u8, 0, 0];
        s.extend_from_slice(body);
        if strings.is_empty() {
            s.extend_from_slice(&[0, 0]);
        } else {
            for st in strings {
                s.extend_from_slice(st.as_bytes());
                s.push(0);
            }
            s.push(0);
        }
        s
    }

    /// Type 17 body (offsets relative to structure start, minus the 4-byte header).
    fn device(size_mb: u16, form: u8, locator_idx: u8, mtype: u8, speed: u16) -> Vec<u8> {
        let mut b = vec![0u8; 0x22 - 4];
        b[0x0C - 4..0x0E - 4].copy_from_slice(&size_mb.to_le_bytes());
        b[0x0E - 4] = form;
        b[0x10 - 4] = locator_idx;
        b[0x12 - 4] = mtype;
        b[0x15 - 4..0x17 - 4].copy_from_slice(&speed.to_le_bytes());
        b
    }

    #[test]
    fn parses_laptop_with_soldered_and_slot() {
        let mut array = vec![0u8; 0x17 - 4];
        array[5 - 4] = 3; // system memory
        array[7 - 4..11 - 4].copy_from_slice(&(32u32 * 1024 * 1024).to_le_bytes()); // 32 GB in KB
        let mut t = structure(16, &array, &[]);
        t.extend(structure(17, &device(4096, 0x0D, 1, 0x1A, 3200), &["Controller0-ChannelA-DIMM0"]));
        t.extend(structure(17, &device(4096, 0x0D, 1, 0x1A, 3200), &["Controller1-ChannelA"]));
        t.extend(structure(127, &[], &[]));
        let info = parse_smbios(&t);
        assert_eq!(info.max_capacity_mb, 32 * 1024);
        assert_eq!(info.modules.len(), 2);
        assert_eq!(info.total_mb(), 8192);
        assert!(!info.modules[0].soldered);
        assert!(info.modules[1].soldered);
        assert_eq!(info.modules[0].kind, "DDR4");
        assert_eq!(info.modules[0].speed_mts, 3200);
        assert_eq!(info.soldered_mb(), 4096);
        assert_eq!(info.removable().len(), 1);
    }

    #[test]
    fn counts_empty_slots_and_survives_garbage() {
        let mut t = structure(17, &device(0, 0x09, 1, 0x1A, 0), &["DIMM_B1"]);
        t.extend(structure(17, &device(8192, 0x09, 1, 0x22, 4800), &["DIMM_A1"]));
        let info = parse_smbios(&t);
        assert_eq!(info.empty_slots, 1);
        assert_eq!(info.modules[0].kind, "DDR5");
        assert_eq!(parse_smbios(&[17, 200, 0]), RamInfo::default());
        assert_eq!(parse_smbios(&[]), RamInfo::default());
    }

    #[test]
    fn reads_this_pc_read_only() {
        assert!(uptime_secs() > 0);
        let (free, total) = disk_space("C:\\").expect("C: exists");
        assert!(total > free && total > 0);
        if let Some(ram) = read_ram() {
            assert!(ram.total_mb() >= 1024, "{ram:?}");
        }
    }
}
