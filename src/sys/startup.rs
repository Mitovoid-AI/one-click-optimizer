//! Run-key startup entries and the Task Manager "StartupApproved" flags.

use crate::model::{RunLocation, StartupEntry};
use crate::util::{from_wide, wide};
use std::ptr::{null, null_mut};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegEnumValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
};

const KEY_READ: u32 = 0x20019;
const KEY_WRITE: u32 = 0x20006;
const KEY_WOW64_64KEY: u32 = 0x100;
const REG_SZ: u32 = 1;
const REG_EXPAND_SZ: u32 = 2;
const REG_BINARY: u32 = 3;
const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_NO_MORE_ITEMS: u32 = 259;

const AGENT_VALUE: &str = "OneClickOptimizer";
/// Name used before the app was renamed.
const LEGACY_AGENT_VALUE: &str = "RamDoctor";

struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

fn root(loc: RunLocation) -> HKEY {
    if loc.is_user() {
        HKEY_CURRENT_USER
    } else {
        HKEY_LOCAL_MACHINE
    }
}

fn open(root: HKEY, path: &str, access: u32) -> Result<Key, u32> {
    let mut k: HKEY = null_mut();
    let r = unsafe { RegOpenKeyExW(root, wide(path).as_ptr(), 0, access | KEY_WOW64_64KEY, &mut k) };
    if r == 0 {
        Ok(Key(k))
    } else {
        Err(r)
    }
}

fn create(root: HKEY, path: &str) -> Result<Key, u32> {
    let mut k: HKEY = null_mut();
    let r = unsafe {
        RegCreateKeyExW(
            root,
            wide(path).as_ptr(),
            0,
            null(),
            0,
            KEY_WRITE | KEY_READ | KEY_WOW64_64KEY,
            null(),
            &mut k,
            null_mut(),
        )
    };
    if r == 0 {
        Ok(Key(k))
    } else {
        Err(r)
    }
}

fn reg_string(data: &[u8]) -> String {
    let words: Vec<u16> = data.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    from_wide(&words)
}

fn string_values(k: &Key) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut name = vec![0u16; 16_384];
    let mut data = vec![0u8; 64 * 1024];
    for i in 0.. {
        let mut name_len = name.len() as u32;
        let mut data_len = data.len() as u32;
        let mut ty = 0u32;
        let r = unsafe {
            RegEnumValueW(k.0, i, name.as_mut_ptr(), &mut name_len, null(), &mut ty, data.as_mut_ptr(), &mut data_len)
        };
        if r == ERROR_NO_MORE_ITEMS {
            break;
        }
        if r != 0 {
            continue;
        }
        if ty == REG_SZ || ty == REG_EXPAND_SZ {
            out.push((
                String::from_utf16_lossy(&name[..name_len as usize]),
                reg_string(&data[..data_len as usize]),
            ));
        }
    }
    out
}

/// `02`/`06` in the first byte = enabled; `03`/`07` = disabled (Task Manager format).
pub fn approved_enabled(bytes: Option<&[u8]>) -> bool {
    bytes.and_then(|b| b.first()).map_or(true, |b| b & 1 == 0)
}

/// Builds the 12-byte StartupApproved value, keeping any extra bytes Windows stored.
pub fn approved_bytes(prev: Option<&[u8]>, enabled: bool, now_ft: u64) -> Vec<u8> {
    let mut b = prev.filter(|p| p.len() >= 12).map(<[u8]>::to_vec).unwrap_or_else(|| vec![0u8; 12]);
    b[0] = if enabled { 2 } else { 3 };
    // Bytes 4..12: when it was disabled (shown by Task Manager); zero when enabled.
    let stamp = if enabled { 0u64 } else { now_ft };
    b[4..12].copy_from_slice(&stamp.to_le_bytes());
    b
}

pub fn read_approved(loc: RunLocation, name: &str) -> Result<Option<Vec<u8>>, u32> {
    let k = match open(root(loc), loc.approved_key(), KEY_READ) {
        Ok(k) => k,
        Err(ERROR_FILE_NOT_FOUND) => return Ok(None),
        Err(e) => return Err(e),
    };
    let wname = wide(name);
    let (mut ty, mut len) = (0u32, 0u32);
    let r = unsafe { RegQueryValueExW(k.0, wname.as_ptr(), null(), &mut ty, null_mut(), &mut len) };
    if r == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if r != 0 {
        return Err(r);
    }
    let mut buf = vec![0u8; len as usize];
    let r = unsafe { RegQueryValueExW(k.0, wname.as_ptr(), null(), &mut ty, buf.as_mut_ptr(), &mut len) };
    if r != 0 {
        return Err(r);
    }
    buf.truncate(len as usize);
    Ok(Some(buf))
}

pub fn write_approved(loc: RunLocation, name: &str, bytes: &[u8]) -> Result<(), u32> {
    let k = create(root(loc), loc.approved_key())?;
    let r = unsafe { RegSetValueExW(k.0, wide(name).as_ptr(), 0, REG_BINARY, bytes.as_ptr(), bytes.len() as u32) };
    if r == 0 {
        Ok(())
    } else {
        Err(r)
    }
}

pub fn delete_approved(loc: RunLocation, name: &str) -> Result<(), u32> {
    let k = match open(root(loc), loc.approved_key(), KEY_WRITE) {
        Ok(k) => k,
        Err(ERROR_FILE_NOT_FOUND) => return Ok(()),
        Err(e) => return Err(e),
    };
    match unsafe { RegDeleteValueW(k.0, wide(name).as_ptr()) } {
        0 | ERROR_FILE_NOT_FOUND => Ok(()),
        e => Err(e),
    }
}

pub fn list_startup() -> Vec<StartupEntry> {
    let mut out = Vec::new();
    for loc in RunLocation::ALL {
        let Ok(k) = open(root(loc), loc.run_key(), KEY_READ) else { continue };
        for (name, command) in string_values(&k) {
            let enabled = approved_enabled(read_approved(loc, &name).ok().flatten().as_deref());
            out.push(StartupEntry { location: loc, name, command, enabled });
        }
    }
    out
}

/// Is the tray agent registered to start with Windows?
pub fn agent_autostart() -> bool {
    open(HKEY_CURRENT_USER, RunLocation::HkcuRun.run_key(), KEY_READ)
        .map(|k| string_values(&k).iter().any(|(n, _)| n == AGENT_VALUE))
        .unwrap_or(false)
}

pub fn set_agent_autostart(on: bool, exe: &str) -> Result<(), u32> {
    let k = create(HKEY_CURRENT_USER, RunLocation::HkcuRun.run_key())?;
    let name = wide(AGENT_VALUE);
    let r = if on {
        let cmd = wide(&format!("\"{exe}\" agent"));
        let bytes = unsafe { std::slice::from_raw_parts(cmd.as_ptr() as *const u8, cmd.len() * 2) };
        unsafe { RegSetValueExW(k.0, name.as_ptr(), 0, REG_SZ, bytes.as_ptr(), bytes.len() as u32) }
    } else {
        match unsafe { RegDeleteValueW(k.0, name.as_ptr()) } {
            ERROR_FILE_NOT_FOUND => 0,
            r => r,
        }
    };
    if r == 0 {
        Ok(())
    } else {
        Err(r)
    }
}

/// Removes the old "RamDoctor" startup entry. Returns true if there was one.
pub fn remove_legacy_autostart() -> bool {
    let Ok(k) = open(HKEY_CURRENT_USER, RunLocation::HkcuRun.run_key(), KEY_READ | KEY_WRITE) else {
        return false;
    };
    let existed = string_values(&k).iter().any(|(n, _)| n == LEGACY_AGENT_VALUE);
    if existed {
        unsafe { RegDeleteValueW(k.0, wide(LEGACY_AGENT_VALUE).as_ptr()) };
    }
    existed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approved_flag_parsing() {
        assert!(approved_enabled(None));
        assert!(approved_enabled(Some(&[2, 0, 0])));
        assert!(approved_enabled(Some(&[6, 0])));
        assert!(!approved_enabled(Some(&[3, 0])));
        assert!(!approved_enabled(Some(&[7])));
    }

    #[test]
    fn approved_bytes_keep_tail() {
        let prev = [6u8, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 0xAA];
        let off = approved_bytes(Some(&prev), false, 0x0102_0304_0506_0708);
        assert_eq!(off.len(), 13);
        assert_eq!(off[0], 3);
        assert_eq!(&off[4..12], &0x0102_0304_0506_0708u64.to_le_bytes());
        assert_eq!(off[12], 0xAA);
        let on = approved_bytes(Some(&off), true, 99);
        assert_eq!(on[0], 2);
        assert_eq!(&on[4..12], &[0u8; 8]);
        assert_eq!(approved_bytes(None, false, 1).len(), 12);
    }

    /// Touches only a made-up value name under the user's StartupApproved key.
    #[test]
    fn approved_write_read_delete_roundtrip() {
        let loc = RunLocation::HkcuRun;
        let name = "OneClick-selftest-entry";
        let _ = delete_approved(loc, name);
        assert_eq!(read_approved(loc, name), Ok(None));
        let bytes = approved_bytes(None, false, 42);
        write_approved(loc, name, &bytes).unwrap();
        assert_eq!(read_approved(loc, name), Ok(Some(bytes)));
        delete_approved(loc, name).unwrap();
        assert_eq!(read_approved(loc, name), Ok(None));
    }

    #[test]
    fn lists_startup_read_only() {
        // Every entry must have a name; this machine has at least one Run entry.
        let all = list_startup();
        assert!(all.iter().all(|e| !e.name.is_empty()));
    }
}
