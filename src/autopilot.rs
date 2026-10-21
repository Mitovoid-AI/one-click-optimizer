//! Autopilot: a scheduled clean-up the tray watch runs on its own, daily or
//! weekly, once the PC has been idle for a few minutes.
//!
//! It only does the safe, quiet part of Optimise: it deletes old temporary
//! files and crash dumps, and closes build daemons (Gradle, Kotlin) that sat
//! idle. Dev servers and adb are closed only if you turn that on. It never
//! closes apps you use, never changes startup apps or services, and never
//! asks for admin rights. Everything it does goes into History.
//!
//! A program is only closed when all of this holds:
//! - it used almost no CPU for the last 30 minutes (measured by the tray
//!   watch every 5 minutes, not guessed from a 2-second sample);
//! - nothing is connected to it (no browser tab on a dev server, no build
//!   talking to a daemon, no phone on a test server);
//! - you haven't marked it "Never close this" in History.

use crate::actions::{self, Action, Journal, Op};
use crate::analyze::{self, Category};
use crate::kb::{self, ProcKind};
use crate::model::Snapshot;
use crate::util::{data_dir, fmt_bytes, journal_path, unix_now};
use crate::{junk, optimize, sys};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Minutes without keyboard or mouse input before Autopilot runs.
pub const IDLE_SECS: u64 = 5 * 60;
/// A program must have been idle this long before Autopilot closes it.
pub const PROGRAM_IDLE_SECS: u64 = 30 * 60;
/// "Idle": less CPU time than this over those 30 minutes (100-ns units: 2 s).
const IDLE_CPU_TICKS: u64 = 2 * 10_000_000;
/// The tray records CPU use this often.
pub const SAMPLE_EVERY_SECS: u64 = 5 * 60;
/// Runs up to an hour early, so "daily" doesn't drift later every day.
const SLACK_SECS: u64 = 3600;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schedule {
    #[serde(default)]
    pub enabled: bool,
    /// 1 = daily, 7 = weekly.
    #[serde(default = "weekly")]
    pub every_days: u32,
    /// Unix seconds of the last run (0 = never).
    #[serde(default)]
    pub last_run: u64,
    #[serde(default)]
    pub last_result: String,
    /// Also close idle dev servers and adb (off: build daemons only).
    #[serde(default)]
    pub close_dev_servers: bool,
}

fn weekly() -> u32 {
    7
}

impl Default for Schedule {
    fn default() -> Self {
        Schedule { enabled: false, every_days: 7, last_run: 0, last_result: String::new(), close_dev_servers: false }
    }
}

impl Schedule {
    pub fn due(&self, now: u64, idle_secs: u64) -> bool {
        self.enabled && idle_secs >= IDLE_SECS && now.saturating_sub(self.last_run) + SLACK_SECS >= self.every_days.max(1) as u64 * 86_400
    }

    pub fn every_text(&self) -> &'static str {
        if self.every_days <= 1 {
            "every day"
        } else {
            "every week"
        }
    }

    /// "Next clean-up: ..." for the Autopilot page.
    pub fn next_text(&self, now: u64) -> String {
        if !self.enabled {
            return "Off".into();
        }
        let next = self.last_run + self.every_days.max(1) as u64 * 86_400;
        if self.last_run == 0 || next <= now {
            "Next clean-up: the next time your PC is idle for 5 minutes".into()
        } else {
            let days = (next - now).div_ceil(86_400);
            if days <= 1 {
                "Next clean-up: within a day, when your PC is idle".into()
            } else {
                format!("Next clean-up: in {days} days, when your PC is idle")
            }
        }
    }
}

pub fn load() -> Schedule {
    crate::settings::load().autopilot
}

pub fn save(s: &Schedule) {
    let s = s.clone();
    crate::settings::update(move |x| x.autopilot = s);
}

// --------------------------------------------------------------- conditions

#[repr(C)]
struct LastInput {
    size: u32,
    time: u32,
}

#[repr(C)]
#[derive(Default)]
struct PowerStatus {
    ac_line: u8,
    battery_flag: u8,
    battery_percent: u8,
    saver: u8,
    life_time: u32,
    full_life_time: u32,
}

#[link(name = "user32")]
extern "system" {
    fn GetLastInputInfo(info: *mut LastInput) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetTickCount() -> u32;
    fn GetSystemPowerStatus(st: *mut PowerStatus) -> i32;
}

#[link(name = "shell32")]
extern "system" {
    fn SHQueryUserNotificationState(state: *mut i32) -> i32;
}

/// Seconds since the last keyboard or mouse input (`OCO_IDLE_SECS` fakes it for tests).
pub fn idle_secs() -> u64 {
    if let Some(v) = std::env::var("OCO_IDLE_SECS").ok().and_then(|v| v.parse().ok()) {
        return v;
    }
    let mut li = LastInput { size: 8, time: 0 };
    if unsafe { GetLastInputInfo(&mut li) } == 0 {
        return 0;
    }
    (unsafe { GetTickCount() }.wrapping_sub(li.time) / 1000) as u64
}

/// Plugged in, or a battery above 30% without battery saver.
pub fn power_ok() -> bool {
    let mut st = PowerStatus::default();
    if unsafe { GetSystemPowerStatus(&mut st) } == 0 {
        return true;
    }
    st.ac_line == 1 || st.battery_flag == 128 /* no battery */ || (st.battery_percent != 255 && st.battery_percent >= 30 && st.saver == 0)
}

/// Not presenting, gaming full-screen or in quiet hours.
pub fn user_not_busy() -> bool {
    let mut state = 0;
    if unsafe { SHQueryUserNotificationState(&mut state) } < 0 {
        return true;
    }
    // QUNS_BUSY 2, QUNS_RUNNING_D3D_FULL_SCREEN 3, QUNS_PRESENTATION_MODE 4, QUNS_QUIET_TIME 6
    !matches!(state, 2 | 3 | 4 | 6)
}

// ------------------------------------------------------- 30 minutes of CPU

/// CPU time per process at one moment: (pid, created, cpu in 100-ns units).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CpuSample {
    pub time: u64,
    pub procs: Vec<(u32, u64, u64)>,
}

fn history_path() -> std::path::PathBuf {
    data_dir().join("cpu-history.json")
}

pub fn load_history() -> Vec<CpuSample> {
    std::fs::read(history_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// Called by the tray every 5 minutes: remembers CPU use per process for
/// the last 2 hours, so "idle" means idle for half an hour.
pub fn record_cpu_sample() {
    let now = unix_now();
    let procs = sys::procs::system_processes().into_iter().filter(|p| p.pid > 4).map(|p| (p.pid, p.created, p.cpu)).collect();
    let mut h = load_history();
    h.retain(|s| now.saturating_sub(s.time) <= 2 * 3600 && s.time <= now);
    h.push(CpuSample { time: now, procs });
    if let Ok(b) = serde_json::to_vec(&h) {
        let _ = std::fs::write(history_path(), b);
    }
}

/// Is there a sample old enough to judge 30 minutes of use?
pub fn history_ready(h: &[CpuSample], now: u64) -> bool {
    h.iter().any(|s| now.saturating_sub(s.time) >= PROGRAM_IDLE_SECS)
}

/// Used less than 2 s of CPU since a sample at least 30 minutes old.
/// Unknown (no such sample, or the process wasn't running then) = not idle.
pub fn idle_for_30_min(h: &[CpuSample], now: u64, pid: u32, created: u64, cpu_now: u64) -> bool {
    let Some(old) = h.iter().filter(|s| now.saturating_sub(s.time) >= PROGRAM_IDLE_SECS).max_by_key(|s| s.time) else {
        return false;
    };
    old.procs.iter().find(|&&(p, c, _)| p == pid && c == created).is_some_and(|&(_, _, cpu_then)| cpu_now.saturating_sub(cpu_then) < IDLE_CPU_TICKS)
}

/// Why a program Autopilot found was left running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kept {
    InUse,
    Busy,
    NotWatchedYet,
}

impl Kept {
    fn text(self) -> &'static str {
        match self {
            Kept::InUse => "in use",
            Kept::Busy => "used in the last 30 minutes",
            Kept::NotWatchedYet => "watched for less than 30 minutes",
        }
    }
}

/// Which idle dev-tool finds Autopilot may close. `kinds` says for each find
/// whether every process in it is a build daemon (Gradle or Kotlin).
pub fn choose(
    finds: &[(String, Vec<(u32, u64)>, bool)],
    close_dev_servers: bool,
    history: &[CpuSample],
    cpu_now: &HashMap<(u32, u64), u64>,
    connected: &HashSet<u32>,
    now: u64,
) -> (Vec<usize>, Vec<(String, Kept)>) {
    let mut close = Vec::new();
    let mut kept = Vec::new();
    for (i, (label, procs, build_daemon)) in finds.iter().enumerate() {
        if !build_daemon && !close_dev_servers {
            continue; // dev servers and adb are only closed when you allow it
        }
        let why = if procs.iter().any(|(pid, _)| connected.contains(pid)) {
            Some(Kept::InUse)
        } else if !history_ready(history, now) {
            Some(Kept::NotWatchedYet)
        } else if !procs.iter().all(|&(pid, created)| cpu_now.get(&(pid, created)).is_some_and(|&cpu| idle_for_30_min(history, now, pid, created, cpu))) {
            Some(Kept::Busy)
        } else {
            None
        };
        match why {
            None => close.push(i),
            Some(k) => kept.push((label.clone(), k)),
        }
    }
    (close, kept)
}

// ---------------------------------------------------------------------- run

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub junk_freed: u64,
    pub closed: Vec<String>,
    pub memory_freed: u64,
    pub failed: usize,
    pub dry_run: bool,
    /// Idle-looking programs left running, and why.
    pub kept: Vec<(String, Kept)>,
}

impl Report {
    pub fn text(&self) -> String {
        let mut parts = Vec::new();
        if self.junk_freed > 0 {
            parts.push(format!("deleted {} of junk files", fmt_bytes(self.junk_freed)));
        }
        if !self.closed.is_empty() {
            let mem = if self.memory_freed > 0 { format!(" ({})", fmt_bytes(self.memory_freed)) } else { String::new() };
            parts.push(format!("closed {}{mem}", self.closed.join(", ")));
        }
        if !self.kept.is_empty() {
            let list: Vec<String> = self.kept.iter().map(|(l, k)| format!("{l} ({})", k.text())).collect();
            parts.push(format!("left {} running", list.join(", ")));
        }
        let mut s = if parts.is_empty() { "Nothing needed cleaning".to_string() } else { capital(&parts.join("; ")) };
        if self.failed > 0 {
            s.push_str(&format!(". {} step(s) didn't work, see History", self.failed));
        }
        if self.dry_run {
            s.push_str(" (test run: nothing was changed)");
        }
        s
    }

    pub fn freed(&self) -> u64 {
        self.junk_freed + self.memory_freed
    }
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
}

/// What Autopilot would do right now.
pub struct Plan {
    pub ops: Vec<Op>,
    pub junk_bytes: u64,
    /// (label, memory) of the programs it will close.
    pub tools: Vec<(String, u64)>,
    pub kept: Vec<(String, Kept)>,
}

/// Works out what to clean (scans junk, samples CPU for 2 s, checks the
/// 30-minute history and open connections).
pub fn plan() -> Plan {
    let junk_thread = std::thread::spawn(junk::scan_all);
    let snap = Snapshot { procs: sys::procs::list_processes(2000, true), now: sys::mem::now_filetime(), self_pid: std::process::id(), ..Default::default() };
    let sch = load();
    let mut recs = analyze::analyze(&snap);
    optimize::apply_keep(&mut recs, &crate::settings::load().keep_running);
    let recs: Vec<_> = recs.into_iter().filter(|r| r.category == Category::DevTool && r.preselect).collect();
    let by_pid: HashMap<u32, &crate::model::ProcInfo> = snap.procs.iter().map(|p| (p.pid, p)).collect();
    let finds: Vec<(String, Vec<(u32, u64)>, bool)> = recs
        .iter()
        .filter_map(|r| match &r.action {
            Action::KillProcesses { label, targets } => {
                let build_daemon = targets.iter().all(|t| {
                    by_pid.get(&t.pid).is_some_and(|p| matches!(kb::classify_process(&p.name, &p.cmdline), Some(ProcKind::GradleDaemon | ProcKind::KotlinDaemon)))
                });
                Some((label.clone(), targets.iter().map(|t| (t.pid, t.created)).collect(), build_daemon))
            }
            _ => None,
        })
        .collect();
    let cpu_now: HashMap<(u32, u64), u64> = sys::procs::system_processes().into_iter().map(|p| ((p.pid, p.created), p.cpu)).collect();
    let (close, kept) = choose(&finds, sch.close_dev_servers, &load_history(), &cpu_now, &sys::net::connected_pids(), unix_now());
    let mut ops = Vec::new();
    let mut tools = Vec::new();
    for i in close {
        let r = &recs[i];
        tools.push((finds[i].0.clone(), r.saves_bytes));
        ops.push(Op::Apply { action: r.action.clone() });
    }
    let mut junk_bytes = 0;
    for f in junk_thread.join().unwrap_or_default().iter().filter(|f| f.default_on && !f.admin && f.bytes > 0) {
        junk_bytes += f.bytes;
        ops.push(optimize::junk_op(f));
    }
    Plan { ops, junk_bytes, tools, kept }
}

/// Runs one clean-up now and records it in the schedule.
pub fn run_now() -> Report {
    let Plan { ops, junk_bytes, tools, kept } = plan();
    let mut rep = Report { dry_run: std::env::var_os("OCO_DRY_RUN").is_some(), kept, ..Default::default() };
    if rep.dry_run {
        rep.junk_freed = junk_bytes;
        rep.closed = tools.iter().map(|(l, _)| l.clone()).collect();
        rep.memory_freed = tools.iter().map(|(_, b)| b).sum();
    } else if !ops.is_empty() {
        let path = journal_path();
        let before = Journal::load(&path).entries.len();
        // Admin-only work is never done by Autopilot: it would need a prompt.
        let _skipped_admin = actions::execute(&path, ops, false);
        let after = Journal::load(&path).entries;
        for e in &after[before.min(after.len())..] {
            match (&e.action, e.ok) {
                (Action::CleanJunk { .. }, true) => rep.junk_freed += e.freed_bytes,
                (Action::KillProcesses { label, .. }, true) => {
                    rep.closed.push(label.clone());
                    rep.memory_freed += tools.iter().find(|(l, _)| l == label).map_or(0, |(_, b)| *b);
                }
                (_, false) => rep.failed += 1,
                _ => {}
            }
        }
    }
    let mut s = load();
    s.last_run = unix_now();
    s.last_result = rep.text();
    save(&s);
    rep
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_when_due_and_idle() {
        let day = 86_400;
        let mut s = Schedule { enabled: true, every_days: 7, ..Default::default() };
        assert!(s.due(100 * day, IDLE_SECS), "never ran: first idle moment");
        assert!(!s.due(100 * day, 30), "not while you are using the PC");
        s.last_run = 100 * day;
        assert!(!s.due(103 * day, IDLE_SECS));
        assert!(s.due(107 * day - 1800, IDLE_SECS), "up to an hour early is fine");
        s.every_days = 1;
        assert!(s.due(101 * day, IDLE_SECS));
        s.enabled = false;
        assert!(!s.due(200 * day, IDLE_SECS));
    }

    fn sample(time: u64, procs: &[(u32, u64, u64)]) -> CpuSample {
        CpuSample { time, procs: procs.to_vec() }
    }

    #[test]
    fn idle_means_idle_for_30_minutes() {
        let now = 10_000;
        let sec = 10_000_000u64;
        let h = vec![sample(now - 40 * 60, &[(10, 1, 100 * sec), (20, 2, 5 * sec)]), sample(now - 5 * 60, &[(10, 1, 100 * sec)])];
        assert!(history_ready(&h, now));
        assert!(idle_for_30_min(&h, now, 10, 1, 101 * sec), "1 s of CPU in 40 minutes");
        assert!(!idle_for_30_min(&h, now, 20, 2, 9 * sec), "4 s of CPU: it was working");
        assert!(!idle_for_30_min(&h, now, 30, 3, 0), "not running 30 minutes ago: unknown");
        assert!(!idle_for_30_min(&h, now, 10, 99, 0), "same PID, different process");
        let young = vec![sample(now - 10 * 60, &[(10, 1, 0)])];
        assert!(!history_ready(&young, now) && !idle_for_30_min(&young, now, 10, 1, 0));
    }

    #[test]
    fn only_quiet_unconnected_build_daemons_by_default() {
        let now = 10_000;
        let sec = 10_000_000u64;
        let h = vec![sample(now - 35 * 60, &[(1, 1, 0), (2, 2, 0), (3, 3, 0), (4, 4, 0)])];
        let cpu: HashMap<(u32, u64), u64> = [((1, 1), sec), ((2, 2), 0), ((3, 3), 50 * sec), ((4, 4), 0)].into_iter().collect();
        let finds = vec![
            ("Gradle daemon".to_string(), vec![(1, 1)], true),
            ("Vite dev server (shop)".to_string(), vec![(2, 2)], false),
            ("Kotlin compile daemon".to_string(), vec![(3, 3)], true),
            ("2 Gradle daemons".to_string(), vec![(4, 4)], true),
        ];
        let connected: HashSet<u32> = [4].into_iter().collect();
        let (close, kept) = choose(&finds, false, &h, &cpu, &connected, now);
        assert_eq!(close, vec![0], "the quiet Gradle daemon only");
        assert_eq!(kept, vec![("Kotlin compile daemon".to_string(), Kept::Busy), ("2 Gradle daemons".to_string(), Kept::InUse)]);
        let (close, _) = choose(&finds, true, &h, &cpu, &connected, now);
        assert_eq!(close, vec![0, 1], "dev servers too once you allow it");
        let (close, kept) = choose(&finds, false, &[], &cpu, &HashSet::new(), now);
        assert!(close.is_empty() && kept.iter().all(|(_, k)| *k == Kept::NotWatchedYet), "nothing closed before 30 minutes of watching");
    }

    #[test]
    fn texts() {
        let day = 86_400;
        let s = Schedule { enabled: true, every_days: 7, last_run: 10 * day, ..Default::default() };
        assert_eq!(s.next_text(11 * day), "Next clean-up: in 6 days, when your PC is idle");
        assert_eq!(s.every_text(), "every week");
        assert_eq!(Schedule::default().next_text(0), "Off");
        let r = Report { junk_freed: 300 << 20, closed: vec!["Gradle daemon".into()], memory_freed: 900 << 20, ..Default::default() };
        assert_eq!(r.text(), "Deleted 300 MB of junk files; closed Gradle daemon (900 MB)");
        assert_eq!(Report::default().text(), "Nothing needed cleaning");
        let r = Report { kept: vec![("Vite dev server (shop)".into(), Kept::InUse)], ..Default::default() };
        assert_eq!(r.text(), "Left Vite dev server (shop) (in use) running");
    }

    #[test]
    fn old_settings_files_still_load() {
        let s: crate::settings::Settings = serde_json::from_str(r#"{"skip_version":null,"free_runs_used":2}"#).unwrap();
        assert_eq!(s.autopilot, Schedule::default());
        assert_eq!(s.free_runs_used, 2);
    }
}
