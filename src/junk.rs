//! Junk files: where they live, how big they are, and how to clean them safely.
//!
//! Safety rules:
//! - Only fixed, known folders are touched, and every root must end in a
//!   known junk folder name (`is_safe_root`). Nothing else is ever deleted.
//! - Reparse points (symlinks, junctions) are never followed or deleted, so a
//!   link inside a temp folder can't lead the cleaner somewhere else.
//! - Files newer than the category's minimum age are kept (temp: 7 days, like
//!   Windows Disk Cleanup). Files in use simply fail to delete and are skipped.
//! - Riskier categories (browser cache, developer caches, Recycle Bin, Windows
//!   Update downloads) are off unless the user ticks them.

use serde::Serialize;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const DAY: u64 = 86_400;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Kind {
    Folders,
    RecycleBin,
}

pub struct Category {
    pub id: &'static str,
    pub name: &'static str,
    pub why: &'static str,
    pub min_age_days: u64,
    pub admin: bool,
    pub default_on: bool,
    pub kind: Kind,
}

pub const CATEGORIES: &[Category] = &[
    Category {
        id: "user_temp",
        name: "Temporary files",
        why: "Leftovers apps put in your temp folder and never cleaned up. Only files untouched for 7+ days are removed.",
        min_age_days: 7,
        admin: false,
        default_on: true,
        kind: Kind::Folders,
    },
    Category {
        id: "crash_dumps",
        name: "Crash dumps & error reports",
        why: "Copies of memory saved when apps crashed. Only useful to developers.",
        min_age_days: 1,
        admin: false,
        default_on: true,
        kind: Kind::Folders,
    },
    Category {
        id: "windows_temp",
        name: "Windows temporary files",
        why: "Windows' own temp folder and system error reports. Files untouched for 7+ days are removed. Needs admin.",
        min_age_days: 7,
        admin: true,
        default_on: true,
        kind: Kind::Folders,
    },
    Category {
        id: "browser_cache",
        name: "Browser cache (Chrome, Edge, Brave)",
        why: "Saved copies of websites. You stay signed in; sites load a little slower the first time.",
        min_age_days: 0,
        admin: false,
        default_on: false,
        kind: Kind::Folders,
    },
    Category {
        id: "dev_cache",
        name: "Developer caches (npm, pip, Yarn)",
        why: "Downloaded packages kept for reuse. They download again when a project needs them.",
        min_age_days: 0,
        admin: false,
        default_on: false,
        kind: Kind::Folders,
    },
    Category {
        id: "update_cache",
        name: "Windows Update downloads",
        why: "Installer files Windows already used. Only files older than 10 days are removed. Needs admin.",
        min_age_days: 10,
        admin: true,
        default_on: false,
        kind: Kind::Folders,
    },
    Category {
        id: "recycle_bin",
        name: "Recycle Bin",
        why: "Files you deleted. Emptying it removes them for good.",
        min_age_days: 0,
        admin: false,
        default_on: false,
        kind: Kind::RecycleBin,
    },
];

pub fn category(id: &str) -> Option<&'static Category> {
    CATEGORIES.iter().find(|c| c.id == id)
}

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from).filter(|p| p.is_absolute())
}

/// Folders a category cleans on this PC.
pub fn roots(id: &str) -> Vec<PathBuf> {
    let local = env_path("LOCALAPPDATA");
    let windir = env_path("SystemRoot").unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let progdata = env_path("ProgramData").unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    let mut v = Vec::new();
    match id {
        "user_temp" => {
            if let Some(t) = env_path("TEMP").or_else(|| local.as_ref().map(|l| l.join("Temp"))) {
                v.push(t);
            }
        }
        "crash_dumps" => {
            if let Some(l) = &local {
                v.push(l.join("CrashDumps"));
                v.push(l.join(r"Microsoft\Windows\WER\ReportArchive"));
                v.push(l.join(r"Microsoft\Windows\WER\ReportQueue"));
            }
        }
        "windows_temp" => {
            v.push(windir.join("Temp"));
            v.push(progdata.join(r"Microsoft\Windows\WER\ReportArchive"));
            v.push(progdata.join(r"Microsoft\Windows\WER\ReportQueue"));
        }
        "browser_cache" => {
            if let Some(l) = &local {
                for vendor in [r"Google\Chrome", r"Microsoft\Edge", r"BraveSoftware\Brave-Browser"] {
                    let base = l.join(vendor).join("User Data");
                    let Ok(entries) = std::fs::read_dir(&base) else { continue };
                    for e in entries.flatten() {
                        let name = e.file_name().to_string_lossy().to_string();
                        if name == "Default" || name.starts_with("Profile ") {
                            let p = e.path();
                            v.push(p.join(r"Cache\Cache_Data"));
                            v.push(p.join("Code Cache"));
                            v.push(p.join("GPUCache"));
                        }
                    }
                }
            }
        }
        "dev_cache" => {
            if let Some(l) = &local {
                v.push(l.join(r"npm-cache\_cacache"));
                v.push(l.join(r"pip\Cache"));
                v.push(l.join(r"Yarn\Cache"));
            }
        }
        "update_cache" => v.push(windir.join(r"SoftwareDistribution\Download")),
        _ => {}
    }
    v
}

/// A root is only cleaned if it is one of the known junk folders.
pub fn is_safe_root(p: &Path) -> bool {
    const SUFFIXES: &[&str] = &[
        r"\temp",
        r"\tmp",
        r"\crashdumps",
        r"\wer\reportarchive",
        r"\wer\reportqueue",
        r"\cache\cache_data",
        r"\code cache",
        r"\gpucache",
        r"\npm-cache\_cacache",
        r"\pip\cache",
        r"\yarn\cache",
        r"\softwaredistribution\download",
    ];
    let lower = p.to_string_lossy().to_lowercase().replace('/', "\\");
    let lower = lower.trim_end_matches('\\');
    p.is_absolute() && p.components().count() >= 3 && SUFFIXES.iter().any(|s| lower.ends_with(s))
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Found {
    pub id: String,
    pub name: String,
    pub why: String,
    pub files: u64,
    pub bytes: u64,
    /// False when Windows wouldn't let us look (admin-only folders).
    pub measurable: bool,
    pub admin: bool,
    pub default_on: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CleanReport {
    pub files: u64,
    pub bytes: u64,
    /// Files in use or protected, left alone.
    pub skipped: u64,
}

fn is_reparse(md: &std::fs::Metadata) -> bool {
    md.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

fn old_enough(md: &std::fs::Metadata, now: SystemTime, min_age: Duration) -> bool {
    match md.modified() {
        Ok(m) => now.duration_since(m).map_or(false, |age| age >= min_age),
        Err(_) => false,
    }
}

/// (files, bytes, readable) that `clean_dir` would remove.
pub fn measure_dir(root: &Path, min_age: Duration, now: SystemTime) -> (u64, u64, bool) {
    let readable = std::fs::read_dir(root).is_ok();
    let (mut files, mut bytes) = (0, 0);
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            // DirEntry metadata comes from the directory listing itself (no
            // file open, no antivirus scan) and does not follow links.
            let Ok(md) = e.metadata() else { continue };
            if is_reparse(&md) {
                continue;
            }
            if md.is_dir() {
                stack.push(e.path());
            } else if old_enough(&md, now, min_age) {
                files += 1;
                bytes += md.len();
            }
        }
    }
    (files, bytes, readable)
}

/// Deletes old-enough files under `root` (never `root` itself). Returns
/// whether `dir` ended up empty because of us.
fn clean_inner(dir: &Path, min_age: Duration, now: SystemTime, rep: &mut CleanReport) -> (bool, bool) {
    let Ok(entries) = std::fs::read_dir(dir) else { return (false, false) };
    let mut empty = true;
    let mut removed_any = false;
    for e in entries.flatten() {
        let path = e.path();
        let Ok(md) = e.metadata() else {
            empty = false;
            continue;
        };
        if is_reparse(&md) {
            empty = false; // never follow or delete links
            continue;
        }
        if md.is_dir() {
            let (sub_empty, sub_removed) = clean_inner(&path, min_age, now, rep);
            let dir_old = old_enough(&md, now, min_age.max(Duration::from_secs(DAY)));
            if sub_empty && (sub_removed || dir_old) && std::fs::remove_dir(&path).is_ok() {
                removed_any = true;
            } else {
                empty = false;
            }
        } else if old_enough(&md, now, min_age) {
            let len = md.len();
            if std::fs::remove_file(&path).is_ok() {
                rep.files += 1;
                rep.bytes += len;
                removed_any = true;
            } else {
                rep.skipped += 1;
                empty = false;
            }
        } else {
            empty = false;
        }
    }
    (empty, removed_any)
}

pub fn clean_dir(root: &Path, min_age: Duration, now: SystemTime) -> CleanReport {
    let mut rep = CleanReport::default();
    if is_safe_root(root) {
        if let Ok(md) = std::fs::symlink_metadata(root) {
            if md.is_dir() && !is_reparse(&md) {
                clean_inner(root, min_age, now, &mut rep);
            }
        }
    }
    rep
}

#[repr(C)]
struct RbInfo {
    cb: u32,
    size: i64,
    items: i64,
}

#[link(name = "shell32")]
extern "system" {
    fn SHQueryRecycleBinW(root: *const u16, info: *mut RbInfo) -> i32;
    fn SHEmptyRecycleBinW(hwnd: *mut std::ffi::c_void, root: *const u16, flags: u32) -> i32;
}

fn recycle_bin() -> Option<(u64, u64)> {
    let mut info = RbInfo { cb: std::mem::size_of::<RbInfo>() as u32, size: 0, items: 0 };
    let hr = unsafe { SHQueryRecycleBinW(std::ptr::null(), &mut info) };
    (hr >= 0).then_some((info.items.max(0) as u64, info.size.max(0) as u64))
}

pub fn scan(c: &Category) -> Found {
    let mut f = Found {
        id: c.id.into(),
        name: c.name.into(),
        why: c.why.into(),
        admin: c.admin,
        default_on: c.default_on,
        measurable: true,
        ..Default::default()
    };
    match c.kind {
        Kind::RecycleBin => match recycle_bin() {
            Some((files, bytes)) => (f.files, f.bytes) = (files, bytes),
            None => f.measurable = false,
        },
        Kind::Folders => {
            let now = SystemTime::now();
            let age = Duration::from_secs(c.min_age_days * DAY);
            for root in roots(c.id).iter().filter(|r| r.is_dir() && is_safe_root(r)) {
                let (files, bytes, readable) = measure_dir(root, age, now);
                f.files += files;
                f.bytes += bytes;
                f.measurable &= readable;
            }
        }
    }
    f
}

/// Measures every category, each on its own thread (they hit different folders).
pub fn scan_all() -> Vec<Found> {
    let trace = std::env::var_os("OCO_TRACE").is_some();
    std::thread::scope(|sc| {
        let handles: Vec<_> = CATEGORIES
            .iter()
            .map(|c| {
                sc.spawn(move || {
                    let t = std::time::Instant::now();
                    let f = scan(c);
                    if trace {
                        eprintln!("junk {:<14} {:>6} ms  {} files  {} bytes", c.id, t.elapsed().as_millis(), f.files, f.bytes);
                    }
                    f
                })
            })
            .collect();
        handles.into_iter().zip(CATEGORIES).map(|(h, c)| h.join().unwrap_or_else(|_| Found { id: c.id.into(), name: c.name.into(), ..Default::default() })).collect()
    })
}

/// Cleans one category. Runs in the elevated helper for admin categories.
pub fn clean(id: &str) -> Result<CleanReport, String> {
    let c = category(id).ok_or_else(|| format!("unknown junk category {id}"))?;
    match c.kind {
        Kind::RecycleBin => {
            let before = recycle_bin().unwrap_or((0, 0));
            // SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND
            let hr = unsafe { SHEmptyRecycleBinW(std::ptr::null_mut(), std::ptr::null(), 7) };
            if hr < 0 && before.0 > 0 {
                return Err(format!("Recycle Bin could not be emptied (0x{hr:08X})"));
            }
            Ok(CleanReport { files: before.0, bytes: before.1, skipped: 0 })
        }
        Kind::Folders => {
            let now = SystemTime::now();
            let age = Duration::from_secs(c.min_age_days * DAY);
            let mut total = CleanReport::default();
            for root in roots(id).iter().filter(|r| r.is_dir()) {
                let r = clean_dir(root, age, now);
                total.files += r.files;
                total.bytes += r.bytes;
                total.skipped += r.skipped;
            }
            Ok(total)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("oco-junk-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        base
    }

    fn file(p: &Path, bytes: usize, age_days: u64) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, vec![7u8; bytes]).unwrap();
        let t = SystemTime::now() - Duration::from_secs(age_days * DAY + 60);
        fs::File::options().write(true).open(p).unwrap().set_modified(t).unwrap();
    }

    #[test]
    fn only_known_junk_folders_are_safe_roots() {
        assert!(is_safe_root(Path::new(r"C:\Users\a\AppData\Local\Temp")));
        assert!(is_safe_root(Path::new(r"C:\Windows\Temp\")));
        assert!(is_safe_root(Path::new(r"C:\Users\a\AppData\Local\Google\Chrome\User Data\Default\Cache\Cache_Data")));
        assert!(!is_safe_root(Path::new(r"C:\Users\a")));
        assert!(!is_safe_root(Path::new(r"C:\")));
        assert!(!is_safe_root(Path::new(r"C:\Windows")));
        assert!(!is_safe_root(Path::new(r"C:\Temp\..\Windows")));
        assert!(!is_safe_root(Path::new(r"Temp")));
        assert!(!is_safe_root(Path::new(r"D:\Projects\cache")));
    }

    #[test]
    fn cleans_only_old_files_and_never_follows_links() {
        let base = fixture("clean");
        let root = base.join("Temp");
        let outside = base.join("Precious");
        file(&root.join("old.log"), 1000, 10);
        file(&root.join("new.log"), 500, 0);
        file(&root.join(r"sub\deep\old.bin"), 2000, 30);
        file(&root.join(r"mixed\old.tmp"), 100, 9);
        file(&root.join(r"mixed\fresh.tmp"), 100, 0);
        file(&outside.join("keep.txt"), 300, 99);
        // A junction inside Temp pointing at a folder that must survive.
        let st = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(root.join("link"))
            .arg(&outside)
            .output()
            .unwrap();
        assert!(st.status.success(), "{st:?}");

        let now = SystemTime::now();
        let week = Duration::from_secs(7 * DAY);
        assert_eq!(measure_dir(&root, week, now), (3, 3100, true));
        let rep = clean_dir(&root, week, now);
        assert_eq!(rep, CleanReport { files: 3, bytes: 3100, skipped: 0 });

        assert!(!root.join("old.log").exists());
        assert!(root.join("new.log").exists());
        assert!(!root.join("sub").exists(), "emptied folders are removed");
        assert!(root.join(r"mixed\fresh.tmp").exists());
        assert!(root.join("link").exists(), "links are left alone");
        assert!(outside.join("keep.txt").exists(), "nothing outside Temp is touched");
        assert!(root.exists(), "the root itself is never removed");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn unsafe_root_is_refused() {
        let base = fixture("refuse");
        let root = base.join("Documents");
        file(&root.join("old.docx"), 100, 50);
        assert_eq!(clean_dir(&root, Duration::ZERO, SystemTime::now()), CleanReport::default());
        assert!(root.join("old.docx").exists());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn categories_are_consistent() {
        for c in CATEGORIES {
            assert!(category(c.id).is_some());
            if c.kind == Kind::Folders {
                for r in roots(c.id) {
                    assert!(is_safe_root(&r), "{} root {} must be a safe root", c.id, r.display());
                }
            }
        }
        // Risky ones are opt-in.
        for id in ["browser_cache", "dev_cache", "recycle_bin", "update_cache"] {
            assert!(!category(id).unwrap().default_on, "{id}");
        }
    }

    #[test]
    fn scan_this_pc_read_only() {
        let all = scan_all();
        assert_eq!(all.len(), CATEGORIES.len());
        let temp = all.iter().find(|f| f.id == "user_temp").unwrap();
        assert!(temp.measurable);
    }
}
