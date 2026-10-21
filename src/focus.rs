//! Focus mode: give the apps you are using all of the PC's power.
//!
//! On: background programs that are busy on the CPU go into Windows'
//! efficiency mode (EcoQoS plus below-normal priority, what Task Manager's
//! "Efficiency mode" does), and the power mode switches to Best performance.
//! Apps with a window on screen, and everything they started, are never
//! slowed. Off: every change is put back.
//!
//! The state is saved in `focus.json`, so a crash or a restart never leaves
//! programs slowed down or the power mode changed.

use crate::model::ProcInfo;
use crate::util::{data_dir, unix_now, wide};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::ffi::c_void;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Slowed {
    pub pid: u32,
    /// Creation FILETIME: guards against touching a recycled PID.
    pub created: u64,
    pub name: String,
    /// Priority class before Focus mode (0 = left as it was).
    pub priority: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    /// Unix seconds when Focus mode was turned on.
    pub since: u64,
    /// Power mode before Focus mode (GUID as 32 hex digits); None = unchanged.
    pub power_prev: Option<String>,
    pub slowed: Vec<Slowed>,
}

impl State {
    pub fn summary(&self) -> String {
        let n = self.slowed.len();
        let apps = match n {
            0 => "no busy background programs right now".to_string(),
            1 => "1 background program in efficiency mode".to_string(),
            n => format!("{n} background programs in efficiency mode"),
        };
        if self.power_prev.is_some() {
            format!("{apps} \u{00B7} power mode: Best performance")
        } else {
            apps
        }
    }
}

/// Never slowed: calls, music, video, recording, anti-cheat, browsers and
/// pieces of the Windows shell.
fn never_slow(name: &str) -> bool {
    const KEEP: &[&str] = &[
        "spotify.exe", "vlc.exe", "wmplayer.exe", "music.ui.exe", "applemusic.exe", "itunes.exe", "foobar2000.exe",
        "zoom.exe", "teams.exe", "ms-teams.exe", "msteams.exe", "discord.exe", "skype.exe", "slack.exe", "webex.exe",
        "obs64.exe", "obs32.exe", "streamlabs obs.exe", "chrome.exe", "msedge.exe", "firefox.exe", "brave.exe",
        "opera.exe", "vivaldi.exe", "msedgewebview2.exe", "searchhost.exe", "lockapp.exe", "applicationframehost.exe",
        "systemsettings.exe", "taskmgr.exe", "easyanticheat.exe", "easyanticheat_eos.exe", "beservice.exe", "vgc.exe",
        "vgtray.exe", "faceit.exe", "faceitclient.exe", "nvcontainer.exe", "nvidia share.exe", "nvidia overlay.exe",
        "radeonsoftware.exe", "amdrsserv.exe", "phoneexperiencehost.exe",
    ];
    KEEP.contains(&name.to_ascii_lowercase().as_str())
}

/// The apps you are using: PIDs with a window on screen, every process
/// with the same name, and everything they started (a build run from your
/// editor, a game's helper processes).
struct OnScreen {
    pids: HashSet<u32>,
    names: HashSet<String>,
}

impl OnScreen {
    fn new(procs: &[ProcInfo], windowed: &HashSet<u32>) -> OnScreen {
        let names = procs.iter().filter(|p| windowed.contains(&p.pid)).map(|p| p.name.to_ascii_lowercase()).collect();
        let mut pids: HashSet<u32> = windowed.clone();
        loop {
            let before = pids.len();
            for p in procs {
                if p.ppid != 0 && pids.contains(&p.ppid) {
                    pids.insert(p.pid);
                }
            }
            if pids.len() == before {
                break;
            }
        }
        OnScreen { pids, names }
    }

    fn contains(&self, pid: u32, name: &str) -> bool {
        self.pids.contains(&pid) || self.names.contains(&name.to_ascii_lowercase())
    }
}

/// Busy background programs to slow down (never the apps on screen).
pub fn pick<'a>(procs: &'a [ProcInfo], windowed: &HashSet<u32>, self_pid: u32, already: &[Slowed]) -> Vec<&'a ProcInfo> {
    let on_screen = OnScreen::new(procs, windowed);
    procs
        .iter()
        .filter(|p| p.pid > 4 && p.pid != self_pid)
        .filter(|p| !on_screen.contains(p.pid, &p.name))
        .filter(|p| !crate::kb::is_protected_process(&p.name) && !never_slow(&p.name))
        .filter(|p| p.cpu_pct >= 0.1)
        .filter(|p| !already.iter().any(|s| s.pid == p.pid && s.created == p.created))
        .collect()
}

// ------------------------------------------------------------ Windows calls

const PROCESS_SET_INFORMATION: u32 = 0x0200;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const NORMAL_PRIORITY_CLASS: u32 = 0x20;
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x4000;
const PROCESS_POWER_THROTTLING: i32 = 4;
const THROTTLE_EXECUTION_SPEED: u32 = 1;

#[repr(C)]
struct Throttling {
    version: u32,
    control_mask: u32,
    state_mask: u32,
}

#[link(name = "kernel32")]
extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
    fn CloseHandle(h: *mut c_void) -> i32;
    fn GetPriorityClass(h: *mut c_void) -> u32;
    fn SetPriorityClass(h: *mut c_void, class: u32) -> i32;
    fn SetProcessInformation(h: *mut c_void, class: i32, info: *const c_void, size: u32) -> i32;
    fn GetProcessTimes(h: *mut c_void, creation: *mut u64, exit: *mut u64, kernel: *mut u64, user: *mut u64) -> i32;
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
}

#[link(name = "user32")]
extern "system" {
    fn EnumWindows(cb: unsafe extern "system" fn(*mut c_void, isize) -> i32, l: isize) -> i32;
    fn IsWindowVisible(h: *mut c_void) -> i32;
    fn IsIconic(h: *mut c_void) -> i32;
    fn GetWindowThreadProcessId(h: *mut c_void, pid: *mut u32) -> u32;
    fn GetWindowTextLengthW(h: *mut c_void) -> i32;
}

#[link(name = "dwmapi")]
extern "system" {
    fn DwmGetWindowAttribute(h: *mut c_void, attr: u32, value: *mut c_void, size: u32) -> i32;
}

struct Proc(*mut c_void);
impl Drop for Proc {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

/// Opens `pid` for changing, but only if it is still the same process.
fn open_same(pid: u32, created: u64) -> Option<Proc> {
    let h = unsafe { OpenProcess(PROCESS_SET_INFORMATION | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if h.is_null() {
        return None;
    }
    let h = Proc(h);
    let (mut c, mut e, mut k, mut u) = (0u64, 0u64, 0u64, 0u64);
    let ok = unsafe { GetProcessTimes(h.0, &mut c, &mut e, &mut k, &mut u) } != 0;
    (ok && (created == 0 || c == created)).then_some(h)
}

fn set_throttle(h: &Proc, on: bool) -> bool {
    let t = Throttling { version: 1, control_mask: if on { THROTTLE_EXECUTION_SPEED } else { 0 }, state_mask: if on { THROTTLE_EXECUTION_SPEED } else { 0 } };
    unsafe { SetProcessInformation(h.0, PROCESS_POWER_THROTTLING, &t as *const _ as *const c_void, std::mem::size_of::<Throttling>() as u32) != 0 }
}

/// Puts one process into efficiency mode (EcoQoS + below-normal priority).
pub fn slow_down(p: &ProcInfo) -> Option<Slowed> {
    let h = open_same(p.pid, p.created)?;
    if !set_throttle(&h, true) {
        return None;
    }
    let prev = unsafe { GetPriorityClass(h.0) };
    let mut priority = 0;
    if prev == NORMAL_PRIORITY_CLASS && unsafe { SetPriorityClass(h.0, BELOW_NORMAL_PRIORITY_CLASS) } != 0 {
        priority = prev;
    }
    Some(Slowed { pid: p.pid, created: p.created, name: p.name.clone(), priority })
}

fn restore(s: &Slowed) -> bool {
    let Some(h) = open_same(s.pid, s.created) else { return false };
    set_throttle(&h, false);
    if s.priority != 0 {
        unsafe { SetPriorityClass(h.0, s.priority) };
    }
    true
}

unsafe extern "system" fn collect_window(h: *mut c_void, l: isize) -> i32 {
    let set = &mut *(l as *mut HashSet<u32>);
    if IsWindowVisible(h) != 0 && IsIconic(h) == 0 && GetWindowTextLengthW(h) > 0 {
        let mut cloaked = 0u32;
        // DWMWA_CLOAKED: suspended Store apps keep "visible" but hidden windows.
        DwmGetWindowAttribute(h, 14, &mut cloaked as *mut u32 as *mut c_void, 4);
        if cloaked == 0 {
            let mut pid = 0;
            GetWindowThreadProcessId(h, &mut pid);
            set.insert(pid);
        }
    }
    1
}

/// PIDs that have a window on screen (not minimised, not hidden).
pub fn windowed_pids() -> HashSet<u32> {
    let mut set = HashSet::new();
    unsafe { EnumWindows(collect_window, &mut set as *mut HashSet<u32> as isize) };
    set
}

// ------------------------------------------------------------- power mode

/// Windows 11 "Best performance" power mode overlay.
const BEST_PERFORMANCE: u128 = 0xded574b5_45a0_4f42_8737_46345c09c238;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Guid {
    d1: u32,
    d2: u16,
    d3: u16,
    d4: [u8; 8],
}

impl Guid {
    fn from_u128(v: u128) -> Guid {
        Guid { d1: (v >> 96) as u32, d2: (v >> 80) as u16, d3: (v >> 64) as u16, d4: (v as u64).to_be_bytes() }
    }
    fn to_u128(self) -> u128 {
        ((self.d1 as u128) << 96) | ((self.d2 as u128) << 80) | ((self.d3 as u128) << 64) | u64::from_be_bytes(self.d4) as u128
    }
}

type GetOverlay = unsafe extern "system" fn(*mut Guid) -> u32;
type SetOverlay = unsafe extern "system" fn(Guid) -> u32;

/// The power-mode slider's functions (Windows 10 1709 and later).
fn overlay_api() -> Option<(GetOverlay, SetOverlay)> {
    unsafe {
        let m = LoadLibraryW(wide("powrprof.dll").as_ptr());
        if m.is_null() {
            return None;
        }
        let get = GetProcAddress(m, b"PowerGetActualOverlayScheme\0".as_ptr());
        let set = GetProcAddress(m, b"PowerSetActiveOverlayScheme\0".as_ptr());
        if get.is_null() || set.is_null() {
            return None;
        }
        Some((std::mem::transmute::<*mut c_void, GetOverlay>(get), std::mem::transmute::<*mut c_void, SetOverlay>(set)))
    }
}

/// Switches to Best performance; returns the previous mode to restore.
fn power_up() -> Option<String> {
    let (get, set) = overlay_api()?;
    let mut prev = Guid::default();
    if unsafe { get(&mut prev) } != 0 || prev.to_u128() == BEST_PERFORMANCE {
        return None;
    }
    (unsafe { set(Guid::from_u128(BEST_PERFORMANCE)) } == 0).then(|| format!("{:032x}", prev.to_u128()))
}

fn power_restore(prev: &str) {
    if let (Some((_, set)), Ok(v)) = (overlay_api(), u128::from_str_radix(prev, 16)) {
        unsafe { set(Guid::from_u128(v)) };
    }
}

// ------------------------------------------------------------------- state

fn path() -> std::path::PathBuf {
    data_dir().join("focus.json")
}

pub fn load() -> Option<State> {
    serde_json::from_slice(&std::fs::read(path()).ok()?).ok()
}

fn save(st: &State) {
    if let Ok(b) = serde_json::to_vec_pretty(st) {
        let _ = std::fs::write(path(), b);
    }
}

pub fn is_on() -> bool {
    path().exists()
}

fn busy_processes() -> Vec<ProcInfo> {
    crate::sys::procs::list_processes(1000, false)
}

/// OCO_DRY_RUN: work out what Focus mode would slow, but change nothing
/// (screenshot tests, demos).
fn dry_run() -> bool {
    std::env::var_os("OCO_DRY_RUN").is_some()
}

fn slow_or_pretend(p: &ProcInfo) -> Option<Slowed> {
    if dry_run() {
        return Some(Slowed { pid: p.pid, created: p.created, name: p.name.clone(), priority: 0 });
    }
    slow_down(p)
}

/// Turns Focus mode on (about a second: CPU use is measured first).
pub fn start() -> State {
    if let Some(st) = load() {
        return st;
    }
    let procs = busy_processes();
    let windowed = windowed_pids();
    let slowed: Vec<Slowed> = pick(&procs, &windowed, std::process::id(), &[]).into_iter().filter_map(slow_or_pretend).collect();
    let power_prev = if dry_run() { overlay_api().map(|_| format!("{:032x}", 0u128)) } else { power_up() };
    let st = State { since: unix_now(), power_prev, slowed };
    save(&st);
    st
}

/// Turns Focus mode off and puts everything back. Returns how many programs
/// were restored.
pub fn stop() -> usize {
    let Some(st) = load() else { return 0 };
    if dry_run() {
        let _ = std::fs::remove_file(path());
        return st.slowed.len();
    }
    let n = st.slowed.iter().filter(|s| restore(s)).count();
    if let Some(prev) = &st.power_prev {
        power_restore(prev);
    }
    let _ = std::fs::remove_file(path());
    n
}

/// While Focus mode is on (every half minute from the tray): programs that
/// now have a window on screen get full speed back, newly busy background
/// programs are slowed.
pub fn refresh() {
    if dry_run() {
        return;
    }
    let Some(mut st) = load() else { return };
    let procs = busy_processes();
    let windowed = windowed_pids();
    let on_screen = OnScreen::new(&procs, &windowed);
    let alive: HashSet<(u32, u64)> = procs.iter().map(|p| (p.pid, p.created)).collect();
    st.slowed.retain(|s| {
        if !alive.contains(&(s.pid, s.created)) {
            return false; // exited
        }
        if on_screen.contains(s.pid, &s.name) {
            restore(s); // you switched to it: full speed again
            return false;
        }
        true
    });
    let more: Vec<Slowed> = pick(&procs, &windowed, std::process::id(), &st.slowed).into_iter().filter_map(slow_down).collect();
    st.slowed.extend(more);
    save(&st);
}

/// After a restart the slowed programs are gone, but the power mode stays:
/// put it back and forget the old state.
pub fn recover_after_restart() {
    let Some(st) = load() else { return };
    let booted = unix_now().saturating_sub(crate::sys::hw::uptime_secs());
    if st.since < booted {
        if let (Some(prev), false) = (&st.power_prev, dry_run()) {
            power_restore(prev);
        }
        let _ = std::fs::remove_file(path());
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, ppid: u32, name: &str, cpu: f32) -> ProcInfo {
        ProcInfo { pid, ppid, name: name.into(), cpu_pct: cpu, created: pid as u64 * 10, ..Default::default() }
    }

    #[test]
    fn only_busy_background_programs_are_slowed() {
        let procs = vec![
            p(100, 1, "game.exe", 40.0),           // on screen
            p(101, 100, "gamehelper.exe", 5.0),    // started by the game
            p(200, 1, "code.exe", 2.0),            // on screen
            p(201, 200, "cargo.exe", 30.0),        // a build started from the editor
            p(202, 201, "rustc.exe", 60.0),        // ...and its child
            p(300, 1, "onedrive.exe", 3.0),        // busy in the background
            p(301, 1, "updater.exe", 0.0),         // idle in the background
            p(400, 1, "chrome.exe", 4.0),          // browsers are never slowed
            p(500, 1, "svchost.exe", 9.0),         // part of Windows
            p(600, 1, "code.exe", 1.0),            // same app as one on screen
            p(700, 1, "oneclickoptimizer.exe", 1.0),
        ];
        let windowed: HashSet<u32> = [100, 200].into_iter().collect();
        let got: Vec<&str> = pick(&procs, &windowed, 700, &[]).iter().map(|p| p.name.as_str()).collect();
        assert_eq!(got, vec!["onedrive.exe"]);
        let already = vec![Slowed { pid: 300, created: 3000, name: "onedrive.exe".into(), priority: 0x20 }];
        assert!(pick(&procs, &windowed, 700, &already).is_empty(), "never slowed twice");
    }

    #[test]
    fn state_roundtrip_and_summary() {
        let st = State { since: 5, power_prev: Some(format!("{:032x}", 0u128)), slowed: vec![Slowed { pid: 1, created: 2, name: "a.exe".into(), priority: 0 }] };
        let back: State = serde_json::from_slice(&serde_json::to_vec(&st).unwrap()).unwrap();
        assert_eq!(back, st);
        assert_eq!(st.summary(), "1 background program in efficiency mode \u{00B7} power mode: Best performance");
        assert_eq!(State::default().summary(), "no busy background programs right now");
    }

    #[test]
    fn guid_conversion() {
        let g = Guid::from_u128(BEST_PERFORMANCE);
        assert_eq!(g.d1, 0xded574b5);
        assert_eq!(g.d2, 0x45a0);
        assert_eq!(g.d4, [0x87, 0x37, 0x46, 0x34, 0x5c, 0x09, 0xc2, 0x38]);
        assert_eq!(g.to_u128(), BEST_PERFORMANCE);
    }
}
