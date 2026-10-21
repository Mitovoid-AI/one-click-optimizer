//! Per-user install and uninstall, no admin needed (like VS Code's user installer).
//!
//! Install puts the exe in %LOCALAPPDATA%\Programs\OneClickOptimizer, adds a
//! Start menu shortcut (and optionally a desktop one), registers the app in
//! Settings > Apps > Installed apps with a working Uninstall button, and can
//! start the tray watch at sign-in. Uninstall removes all of it.

use crate::util::wide;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use windows_sys::core::GUID;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER,
};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const EXE_NAME: &str = "OneClickOptimizer.exe";
pub const PRODUCT_KEY: &str = "OneClickOptimizer";
const UNINSTALL_ROOT: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
const KEY_READ: u32 = 0x20019;
const KEY_ALL: u32 = 0xF003F;
const REG_SZ: u32 = 1;
const REG_DWORD: u32 = 4;

const CLSID_SHELL_LINK: GUID = GUID::from_u128(0x00021401_0000_0000_c000_000000000046);
const IID_ISHELL_LINK_W: GUID = GUID::from_u128(0x000214f9_0000_0000_c000_000000000046);
const IID_IPERSIST_FILE: GUID = GUID::from_u128(0x0000010b_0000_0000_c000_000000000046);
const FOLDERID_DESKTOP: GUID = GUID::from_u128(0xb4bfcc3a_db2c_424c_b029_7fe99a87c641);
const FOLDERID_PROGRAMS: GUID = GUID::from_u128(0xa77f5d77_2e2b_44c3_a6a2_aba601054a51);

#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(reserved: *const c_void, coinit: u32) -> i32;
    fn CoUninitialize();
    fn CoCreateInstance(clsid: *const GUID, outer: *mut c_void, ctx: u32, iid: *const GUID, out: *mut *mut c_void) -> i32;
    fn CoTaskMemFree(p: *const c_void);
}

#[link(name = "shell32")]
extern "system" {
    fn SHGetKnownFolderPath(id: *const GUID, flags: u32, token: *mut c_void, path: *mut *mut u16) -> i32;
}

/// Where everything goes. Tests use a throwaway layout.
#[derive(Clone, Debug)]
pub struct Layout {
    pub dir: PathBuf,
    pub start_menu_lnk: PathBuf,
    pub desktop_lnk: PathBuf,
    /// Name of the key under ...\CurrentVersion\Uninstall.
    pub product_key: String,
    /// The real install: closes running copies and manages the "start tray
    /// watch at sign-in" entry. Off in tests, so they never touch the real app.
    pub live: bool,
}

impl Layout {
    /// The real per-user layout, or a throwaway one when OCO_INSTALL_ROOT is
    /// set (used by the setup screen tests: nothing real is touched).
    pub fn standard() -> Layout {
        if let Some(root) = std::env::var_os("OCO_INSTALL_ROOT").map(PathBuf::from).filter(|p| p.is_absolute()) {
            return Layout {
                dir: root.join(r"Programs\OneClickOptimizer"),
                start_menu_lnk: root.join(r"StartMenu\OneClick Optimizer.lnk"),
                desktop_lnk: root.join(r"Desktop\OneClick Optimizer.lnk"),
                product_key: "OneClickOptimizer-Test".into(),
                live: false,
            };
        }
        let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let programs = known_folder(&FOLDERID_PROGRAMS).unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()).join(r"Microsoft\Windows\Start Menu\Programs")
        });
        let desktop = known_folder(&FOLDERID_DESKTOP)
            .unwrap_or_else(|| PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default()).join("Desktop"));
        Layout {
            dir: local.join(r"Programs\OneClickOptimizer"),
            start_menu_lnk: programs.join("OneClick Optimizer.lnk"),
            desktop_lnk: desktop.join("OneClick Optimizer.lnk"),
            product_key: PRODUCT_KEY.into(),
            live: true,
        }
    }

    pub fn exe(&self) -> PathBuf {
        self.dir.join(EXE_NAME)
    }

    fn uninstall_key(&self) -> String {
        format!(r"{UNINSTALL_ROOT}\{}", self.product_key)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub desktop_shortcut: bool,
    pub tray_at_startup: bool,
}

fn known_folder(id: &GUID) -> Option<PathBuf> {
    unsafe {
        let mut p: *mut u16 = null_mut();
        if SHGetKnownFolderPath(id, 0, null_mut(), &mut p) < 0 || p.is_null() {
            return None;
        }
        let s = crate::util::from_wide_ptr(p);
        CoTaskMemFree(p as *const c_void);
        Some(PathBuf::from(s))
    }
}

// ----------------------------------------------------------------- registry

struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

fn create_key(path: &str) -> Result<Key, String> {
    let mut k: HKEY = null_mut();
    let r = unsafe { RegCreateKeyExW(HKEY_CURRENT_USER, wide(path).as_ptr(), 0, null(), 0, KEY_ALL, null(), &mut k, null_mut()) };
    if r == 0 {
        Ok(Key(k))
    } else {
        Err(format!("couldn't write the registry (error {r})"))
    }
}

fn set_str(k: &Key, name: &str, value: &str) {
    let v = wide(value);
    unsafe { RegSetValueExW(k.0, wide(name).as_ptr(), 0, REG_SZ, v.as_ptr() as *const u8, (v.len() * 2) as u32) };
}

fn set_dword(k: &Key, name: &str, value: u32) {
    unsafe { RegSetValueExW(k.0, wide(name).as_ptr(), 0, REG_DWORD, &value as *const u32 as *const u8, 4) };
}

fn get_str(path: &str, name: &str) -> Option<String> {
    unsafe {
        let mut k: HKEY = null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, wide(path).as_ptr(), 0, KEY_READ, &mut k) != 0 {
            return None;
        }
        let k = Key(k);
        let mut buf = vec![0u16; 1024];
        let mut len = (buf.len() * 2) as u32;
        let mut ty = 0u32;
        if RegQueryValueExW(k.0, wide(name).as_ptr(), null(), &mut ty, buf.as_mut_ptr() as *mut u8, &mut len) != 0 || ty != REG_SZ {
            return None;
        }
        Some(crate::util::from_wide(&buf[..(len as usize / 2).min(buf.len())]))
    }
}

/// Installed version, if any.
pub fn installed_version(l: &Layout) -> Option<String> {
    get_str(&l.uninstall_key(), "DisplayVersion")
}

/// True when this process runs from the install folder.
pub fn running_installed_copy(l: &Layout) -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_string_lossy().eq_ignore_ascii_case(&l.dir.to_string_lossy())))
        .unwrap_or(false)
}

// ---------------------------------------------------------------- shortcuts

/// Creates a .lnk with IShellLinkW / IPersistFile (raw COM vtable calls).
pub fn create_shortcut(lnk: &Path, target: &Path, args: &str, description: &str) -> Result<(), String> {
    type SetStr = unsafe extern "system" fn(*mut c_void, *const u16) -> i32;
    type SetIcon = unsafe extern "system" fn(*mut c_void, *const u16, i32) -> i32;
    type Query = unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32;
    type Release = unsafe extern "system" fn(*mut c_void) -> u32;
    type Save = unsafe extern "system" fn(*mut c_void, *const u16, i32) -> i32;
    unsafe {
        let init = CoInitializeEx(null(), 2); // apartment-threaded
        let mut link: *mut c_void = null_mut();
        let hr = CoCreateInstance(&CLSID_SHELL_LINK, null_mut(), 1, &IID_ISHELL_LINK_W, &mut link);
        if hr < 0 || link.is_null() {
            if init >= 0 {
                CoUninitialize();
            }
            return Err(format!("couldn't create a shortcut (0x{hr:08X})"));
        }
        let vt = *(link as *const *const usize);
        let call = |idx: usize, s: &str| {
            let f: SetStr = std::mem::transmute(*vt.add(idx));
            f(link, wide(s).as_ptr())
        };
        let target_s = target.to_string_lossy().to_string();
        call(20, &target_s); // SetPath
        call(11, args); // SetArguments
        call(7, description); // SetDescription
        call(9, &target.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default()); // SetWorkingDirectory
        let set_icon: SetIcon = std::mem::transmute(*vt.add(17));
        set_icon(link, wide(&target_s).as_ptr(), 0);
        let query: Query = std::mem::transmute(*vt.add(0));
        let mut file: *mut c_void = null_mut();
        let mut result = Err("couldn't save the shortcut".to_string());
        if query(link, &IID_IPERSIST_FILE, &mut file) >= 0 && !file.is_null() {
            let fvt = *(file as *const *const usize);
            let save: Save = std::mem::transmute(*fvt.add(6));
            if let Some(dir) = lnk.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if save(file, wide(&lnk.to_string_lossy()).as_ptr(), 1) >= 0 {
                result = Ok(());
            }
            let rel: Release = std::mem::transmute(*fvt.add(2));
            rel(file);
        }
        let rel: Release = std::mem::transmute(*vt.add(2));
        rel(link);
        if init >= 0 {
            CoUninitialize();
        }
        result
    }
}

// ------------------------------------------------------------- running copies

/// Politely closes the installed app (window + tray), then ends any copy of
/// the installed exe that is still running (never this process).
pub fn close_running(l: &Layout) {
    if !l.live {
        return;
    }
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW};
    for class in [crate::ui::CLASS, crate::agent::TRAY_CLASS] {
        let h = unsafe { FindWindowW(wide(class).as_ptr(), null()) };
        if !h.is_null() {
            unsafe { PostMessageW(h, 0x10, 0, 0) }; // WM_CLOSE
        }
    }
    let target = l.exe().to_string_lossy().to_lowercase();
    let me = std::process::id();
    for _ in 0..30 {
        let left: Vec<_> = crate::sys::procs::list_processes(0, true)
            .into_iter()
            .filter(|p| p.pid != me && p.path.to_lowercase() == target)
            .collect();
        if left.is_empty() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    for p in crate::sys::procs::list_processes(0, true).into_iter().filter(|p| p.pid != me && p.path.to_lowercase() == target) {
        let _ = crate::sys::procs::terminate(p.pid, p.created);
    }
}

/// Removes leftovers from an update (`*.old` copies of a running exe).
pub fn cleanup_old_copies(l: &Layout) {
    if let Ok(entries) = std::fs::read_dir(&l.dir) {
        for e in entries.flatten() {
            if e.file_name().to_string_lossy().ends_with(".old") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

/// Copies `src` over `dst`; if `dst` is locked (still running), renames it
/// out of the way first (Windows allows renaming a running exe).
fn put_file(src: &Path, dst: &Path) -> Result<(), String> {
    if src == dst {
        return Ok(());
    }
    if std::fs::copy(src, dst).is_ok() {
        return Ok(());
    }
    if dst.exists() {
        let aside = dst.with_extension(format!("{}.old", std::process::id()));
        std::fs::rename(dst, &aside).map_err(|e| format!("couldn't replace {}: {e}", dst.display()))?;
    }
    std::fs::copy(src, dst).map(|_| ()).map_err(|e| format!("couldn't copy to {}: {e}", dst.display()))
}

fn today() -> String {
    use windows_sys::Win32::Foundation::SYSTEMTIME;
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    let mut st: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut st) };
    format!("{:04}{:02}{:02}", st.wYear, st.wMonth, st.wDay)
}

// --------------------------------------------------------------- install

/// Installs `src_exe` (normally this very exe) into `l`.
pub fn install(src_exe: &Path, l: &Layout, o: &Options) -> Result<(), String> {
    std::fs::create_dir_all(&l.dir).map_err(|e| format!("couldn't create {}: {e}", l.dir.display()))?;
    close_running(l);
    let exe = l.exe();
    put_file(src_exe, &exe)?;
    // The command-line tool comes along when it sits next to the installer.
    if let Some(cli) = src_exe.parent().map(|d| d.join("oneclick-cli.exe")) {
        if cli.exists() {
            let _ = put_file(&cli, &l.dir.join("oneclick-cli.exe"));
        }
    }

    create_shortcut(&l.start_menu_lnk, &exe, "", "Make your PC faster in one click")?;
    if o.desktop_shortcut {
        create_shortcut(&l.desktop_lnk, &exe, "", "Make your PC faster in one click")?;
    } else {
        let _ = std::fs::remove_file(&l.desktop_lnk);
    }

    let k = create_key(&l.uninstall_key())?;
    let exe_s = exe.to_string_lossy();
    set_str(&k, "DisplayName", crate::APP_NAME);
    set_str(&k, "DisplayVersion", VERSION);
    set_str(&k, "Publisher", crate::APP_NAME);
    set_str(&k, "DisplayIcon", &format!("{exe_s},0"));
    set_str(&k, "InstallLocation", &l.dir.to_string_lossy());
    set_str(&k, "UninstallString", &format!("\"{exe_s}\" --uninstall"));
    set_str(&k, "QuietUninstallString", &format!("\"{exe_s}\" --uninstall --quiet"));
    set_str(&k, "InstallDate", &today());
    let size_kb = std::fs::metadata(&exe).map(|m| (m.len() / 1024) as u32).unwrap_or(0);
    set_dword(&k, "EstimatedSize", size_kb);
    set_dword(&k, "NoModify", 1);
    set_dword(&k, "NoRepair", 1);

    if l.live {
        crate::sys::startup::set_agent_autostart(o.tray_at_startup, &exe_s).map_err(|e| format!("couldn't set up the tray watch (error {e})"))?;
    }
    Ok(())
}

/// The install folder must be ours before we delete it.
fn is_safe_install_dir(dir: &Path) -> bool {
    let s = dir.to_string_lossy().to_lowercase();
    dir.is_absolute() && (s.ends_with(r"\programs\oneclickoptimizer") || s.contains("oco-install-test"))
}

/// Removes shortcuts, the Settings entry, the startup entry and the files.
/// When this process runs from the install folder, the folder is removed by a
/// short-lived helper after we exit.
pub fn uninstall(l: &Layout, remove_data: bool) -> Result<(), String> {
    close_running(l);
    if l.live {
        let _ = crate::sys::startup::set_agent_autostart(false, "");
    }
    let _ = std::fs::remove_file(&l.start_menu_lnk);
    let _ = std::fs::remove_file(&l.desktop_lnk);
    unsafe {
        let r = RegDeleteTreeW(HKEY_CURRENT_USER, wide(&l.uninstall_key()).as_ptr());
        if r != 0 && r != 2 {
            return Err(format!("couldn't remove the Settings entry (error {r})"));
        }
    }
    if remove_data && l.live {
        let data = crate::util::data_dir();
        let _ = std::fs::remove_dir_all(data);
    }
    if !is_safe_install_dir(&l.dir) {
        return Err(format!("refusing to delete unexpected folder {}", l.dir.display()));
    }
    if running_installed_copy(l) {
        // We can't delete our own exe while running: a hidden cmd waits for us
        // to exit, then removes the folder.
        use std::os::windows::process::CommandExt;
        // raw_arg: cmd.exe doesn't understand the \" escaping normal args get.
        let cmd = format!("/c ping -n 3 127.0.0.1 >nul & rmdir /s /q \"{}\"", l.dir.display());
        std::process::Command::new("cmd")
            .raw_arg(cmd)
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .spawn()
            .map_err(|e| format!("couldn't schedule folder removal: {e}"))?;
    } else if l.dir.exists() {
        std::fs::remove_dir_all(&l.dir).map_err(|e| format!("couldn't remove {}: {e}", l.dir.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_layout(tag: &str) -> Layout {
        let base = std::env::temp_dir().join(format!("oco-install-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        Layout {
            dir: base.join(r"Programs\OneClickOptimizer"),
            start_menu_lnk: base.join(r"StartMenu\OneClick Optimizer.lnk"),
            desktop_lnk: base.join(r"Desktop\OneClick Optimizer.lnk"),
            product_key: format!("OneClickOptimizer-Test-{}", std::process::id()),
            live: false,
        }
    }

    fn is_lnk(p: &Path) -> bool {
        std::fs::read(p).map(|b| b.len() > 76 && b[..4] == [0x4C, 0, 0, 0]).unwrap_or(false)
    }

    #[test]
    fn standard_layout_is_per_user() {
        let l = Layout::standard();
        let local = std::env::var("LOCALAPPDATA").unwrap().to_lowercase();
        assert!(l.dir.to_string_lossy().to_lowercase().starts_with(&local));
        assert!(l.dir.ends_with(r"Programs\OneClickOptimizer"));
        assert!(l.start_menu_lnk.to_string_lossy().contains(r"Start Menu\Programs"));
        assert!(is_safe_install_dir(&l.dir));
        assert!(!is_safe_install_dir(Path::new(r"C:\Users\someone")));
    }

    /// Installs a dummy exe into a throwaway layout (a test-only Settings
    /// entry is created and removed again), then uninstalls it.
    #[test]
    fn install_then_uninstall_roundtrip() {
        let l = test_layout("roundtrip");
        let src_dir = l.dir.parent().unwrap().parent().unwrap().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        let src = src_dir.join("OneClickOptimizer-Setup.exe");
        std::fs::write(&src, vec![0x4D, 0x5A, 0, 0, 1, 2, 3]).unwrap();

        install(&src, &l, &Options { desktop_shortcut: true, tray_at_startup: false }).unwrap();
        assert!(l.exe().exists(), "exe copied under its app name");
        assert!(is_lnk(&l.start_menu_lnk), "start menu shortcut");
        assert!(is_lnk(&l.desktop_lnk), "desktop shortcut");
        assert_eq!(installed_version(&l).as_deref(), Some(VERSION));
        assert_eq!(get_str(&l.uninstall_key(), "DisplayName").as_deref(), Some(crate::APP_NAME));
        assert!(get_str(&l.uninstall_key(), "UninstallString").unwrap().ends_with("--uninstall"));

        // Re-install without the desktop shortcut removes it.
        install(&src, &l, &Options { desktop_shortcut: false, tray_at_startup: false }).unwrap();
        assert!(!l.desktop_lnk.exists());

        uninstall(&l, false).unwrap();
        assert!(!l.dir.exists());
        assert!(!l.start_menu_lnk.exists());
        assert_eq!(installed_version(&l), None);
        let _ = std::fs::remove_dir_all(l.dir.parent().unwrap().parent().unwrap());
    }
}
