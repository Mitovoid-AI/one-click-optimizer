use std::path::PathBuf;

/// 100-ns FILETIME ticks per second.
pub const FT_PER_SEC: u64 = 10_000_000;

/// NUL-terminated UTF-16 for Win32 calls.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// UTF-16 buffer (possibly NUL-terminated) to String.
pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// # Safety
/// `p` must be null or point to a NUL-terminated UTF-16 string.
pub unsafe fn from_wide_ptr(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut n = 0;
    while *p.add(n) != 0 {
        n += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(p, n))
}

pub fn fmt_bytes(b: u64) -> String {
    let gb = b as f64 / (1u64 << 30) as f64;
    if gb >= 1.0 {
        format!("{gb:.1} GB")
    } else if b >= 1 << 20 {
        format!("{} MB", b >> 20)
    } else {
        format!("{} KB", (b >> 10).max(1))
    }
}

pub fn fmt_duration(secs: u64) -> String {
    let (d, h, m) = (secs / 86_400, (secs % 86_400) / 3600, (secs % 3600) / 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m")
    } else {
        "under a minute".to_string()
    }
}

pub fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 15) as usize] as char);
    }
    s
}

pub fn hex_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

/// %LOCALAPPDATA%\OneClickOptimizer (created on demand). The folder of the
/// app's earlier name (RamDoctor) is moved over once, keeping the history.
pub fn data_dir() -> PathBuf {
    // OCO_DATA_DIR: tests keep their history/settings away from the real ones.
    if let Some(d) = std::env::var_os("OCO_DATA_DIR").map(PathBuf::from).filter(|p| p.is_absolute()) {
        let _ = std::fs::create_dir_all(&d);
        return d;
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("OneClickOptimizer");
    let old = base.join("RamDoctor");
    if !dir.exists() && old.is_dir() {
        let _ = std::fs::rename(&old, &dir);
    }
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn journal_path() -> PathBuf {
    data_dir().join("journal.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_format() {
        assert_eq!(fmt_bytes(512 << 20), "512 MB");
        assert_eq!(fmt_bytes(3 << 30), "3.0 GB");
        assert_eq!(fmt_bytes(1_503_238_553), "1.4 GB");
        assert_eq!(fmt_bytes(969 * 1024), "969 KB");
        assert_eq!(fmt_bytes(1), "1 KB");
    }

    #[test]
    fn duration_format() {
        assert_eq!(fmt_duration(30), "under a minute");
        assert_eq!(fmt_duration(5 * 60), "5m");
        assert_eq!(fmt_duration(3 * 3600 + 12 * 60), "3h 12m");
        assert_eq!(fmt_duration(2 * 86_400 + 3600), "2d 1h");
    }

    #[test]
    fn hex_roundtrip() {
        let data = b"{\"ops\":[1,2,3]} \x00\xff";
        let h = hex_encode(data);
        assert_eq!(hex_decode(&h).unwrap(), data);
        assert!(hex_decode("abc").is_none());
        assert!(hex_decode("zz").is_none());
    }

    #[test]
    fn wide_roundtrip() {
        let w = wide("héllo");
        assert_eq!(*w.last().unwrap(), 0);
        assert_eq!(from_wide(&w), "héllo");
    }
}
