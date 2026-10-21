//! The one-click engine shared by the window and the tray popup:
//! deep scan -> default plan -> run (with one admin prompt if needed) -> summary.

use crate::actions::{self, Action, Journal, Op, Plan};
use crate::analyze::{self, Category, Recommendation};
use crate::elevate::{self, ElevateError};
use crate::health::{self, Facts, Insight};
use crate::junk::{self, Found};
use crate::model::Snapshot;
use crate::sys;
use crate::util::{fmt_bytes, journal_path};

pub const SCAN_STEPS: [&str; 4] = [
    "Memory and running programs",
    "Junk files",
    "Startup apps and services",
    "Health checks",
];

pub const RUN_STEPS: [&str; 4] = [
    "Closing idle programs",
    "Cleaning junk files",
    "Startup apps and services",
    "Measuring the result",
];

#[derive(Clone, Debug)]
pub struct ScanResult {
    pub snap: Snapshot,
    pub recs: Vec<Recommendation>,
    pub junk: Vec<Found>,
    pub insights: Vec<Insight>,
    pub facts: Facts,
    pub score: u8,
    pub score_after: u8,
}

impl ScanResult {
    /// Memory the pre-ticked fixes free right now.
    pub fn fix_bytes(&self) -> u64 {
        self.recs.iter().filter(|r| r.preselect).map(|r| r.saves_bytes).sum()
    }
    /// Junk the default categories would remove.
    pub fn junk_bytes(&self) -> u64 {
        self.junk.iter().filter(|j| j.default_on).map(|j| j.bytes).sum()
    }
    pub fn idle_programs(&self) -> usize {
        self.recs.iter().filter(|r| matches!(r.category, Category::DevTool | Category::BackgroundHelper)).count()
    }
    pub fn startup_items(&self) -> usize {
        self.recs.iter().filter(|r| matches!(r.category, Category::Startup | Category::Service)).count()
    }
    pub fn warnings(&self) -> usize {
        self.insights.iter().filter(|i| i.severity != health::Severity::Tip).count()
    }
}

/// Full scan. `progress(step)` is called as each of `SCAN_STEPS` starts.
pub fn deep_scan(progress: &dyn Fn(usize)) -> ScanResult {
    progress(0);
    // Junk folders are measured while the 2 s process sample runs.
    let junk_thread = std::thread::spawn(junk::scan_all);
    // 2 s sample: CPU use per process and memory growth (leak hints).
    let snap = sys::snapshot(2000);
    progress(1);
    let junk = junk_thread.join().unwrap_or_default();
    progress(2);
    let mut recs = analyze::analyze(&snap);
    apply_keep(&mut recs, &crate::settings::load().keep_running);
    progress(3);
    let facts = Facts {
        uptime_secs: sys::hw::uptime_secs(),
        disk_c: sys::hw::disk_space("C:\\"),
        ram: sys::hw::read_ram(),
    };
    let insights = health::insights(&snap, &facts);
    let junk_default: u64 = junk.iter().filter(|j| j.default_on).map(|j| j.bytes).sum();
    let score = health::score(&snap.mem, &recs, junk_default, &insights);
    let freed: u64 = recs.iter().filter(|r| r.preselect).map(|r| r.saves_bytes).sum();
    let lasting: Vec<Insight> = insights.iter().filter(|i| !i.title.starts_with("Restart")).cloned().collect();
    let score_after = health::score_after(&snap.mem, freed, &lasting).max(score);
    ScanResult { snap, recs, junk, insights, facts, score, score_after }
}

/// Stable name for "never close this": "2 Gradle daemons" and "Gradle daemon"
/// are the same thing to keep.
pub fn keep_key(label: &str) -> String {
    let mut s = label.trim().to_ascii_lowercase();
    if let Some((first, rest)) = s.split_once(' ') {
        if first.chars().all(|c| c.is_ascii_digit()) {
            s = rest.to_string();
        }
    }
    s.replace("daemons", "daemon").replace("servers", "server")
}

/// Programs you chose to keep running are never ticked for one-click Optimise.
pub fn apply_keep(recs: &mut [Recommendation], keep: &[String]) {
    for r in recs.iter_mut() {
        if let Action::KillProcesses { label, .. } = &r.action {
            if keep.contains(&keep_key(label)) {
                r.preselect = false;
                r.why.push_str(" You chose to keep it running, so it isn't ticked.");
            }
        }
    }
}

/// "frees 1.5 GB RAM · 1.3 GB disk" (memory and disk space kept apart).
pub fn frees_text(ram: u64, disk: u64) -> Option<String> {
    match (ram > 0, disk > 0) {
        (true, true) => Some(format!("frees {} RAM \u{00B7} {} disk", fmt_bytes(ram), fmt_bytes(disk))),
        (true, false) => Some(format!("frees {} of RAM", fmt_bytes(ram))),
        (false, true) => Some(format!("frees {} of disk", fmt_bytes(disk))),
        (false, false) => None,
    }
}

pub fn junk_op(f: &Found) -> Op {
    Op::Apply { action: Action::CleanJunk { id: f.id.clone(), label: f.name.to_lowercase() } }
}

/// What one click does: every pre-ticked fix plus default junk categories
/// that actually contain something.
pub fn default_ops(r: &ScanResult) -> Vec<Op> {
    let mut ops: Vec<Op> = r.recs.iter().filter(|x| x.preselect).map(|x| Op::Apply { action: x.action.clone() }).collect();
    ops.extend(r.junk.iter().filter(|j| j.default_on && (j.bytes > 0 || (!j.measurable && j.admin))).map(junk_op));
    ops
}

/// Short bullet lines describing a plan (for the popup and confirmations).
pub fn plan_lines(r: &ScanResult, ops: &[Op]) -> Vec<String> {
    let mut kills = 0usize;
    let mut kill_bytes = 0u64;
    let mut junk_bytes = 0u64;
    let mut junk_n = 0usize;
    let mut startup = 0usize;
    let mut services = 0usize;
    for op in ops {
        let Op::Apply { action } = op else { continue };
        match action {
            Action::KillProcesses { .. } => {
                kills += 1;
                kill_bytes += r.recs.iter().find(|x| &x.action == action).map_or(0, |x| x.saves_bytes);
            }
            Action::CleanJunk { id, .. } => {
                junk_n += 1;
                junk_bytes += r.junk.iter().find(|j| &j.id == id).map_or(0, |j| j.bytes);
            }
            Action::DeleteDevFolder { .. } => junk_n += 1,
            Action::SetStartupEnabled { .. } => startup += 1,
            Action::SetServiceStart { .. } => services += 1,
        }
    }
    let mut v = Vec::new();
    if kills > 0 {
        v.push(format!("Close {kills} idle program(s) \u{2014} frees {}", fmt_bytes(kill_bytes)));
    }
    if junk_n > 0 {
        v.push(if junk_bytes > 0 {
            format!("Delete {} of junk files", fmt_bytes(junk_bytes))
        } else {
            "Clean Windows junk files".to_string()
        });
    }
    if startup > 0 {
        v.push(format!("Stop {startup} app(s) opening at sign-in"));
    }
    if services > 0 {
        v.push(format!("Start {services} background service(s) only when needed"));
    }
    v
}

#[derive(Clone, Debug, Default)]
pub struct RunSummary {
    pub applied: usize,
    pub failed: Vec<String>,
    pub closed: usize,
    pub junk_freed: u64,
    pub startup_off: usize,
    pub services: usize,
    pub cancelled_admin: usize,
    pub mem_before: u64,
    pub mem_after: u64,
    /// OCO_DRY_RUN was set: nothing was changed (used for tests and demos).
    pub dry_run: bool,
}

impl RunSummary {
    pub fn mem_freed(&self) -> u64 {
        self.mem_before.saturating_sub(self.mem_after)
    }

    pub fn lines(&self) -> Vec<String> {
        let mut v = Vec::new();
        if self.mem_freed() > 0 {
            v.push(format!("Freed {} of memory", fmt_bytes(self.mem_freed())));
        }
        if self.closed > 0 {
            v.push(format!("Closed {} idle program group(s)", self.closed));
        }
        if self.junk_freed > 0 {
            v.push(format!("Deleted {} of junk files", fmt_bytes(self.junk_freed)));
        }
        if self.startup_off > 0 {
            v.push(format!("{} app(s) no longer open at sign-in", self.startup_off));
        }
        if self.services > 0 {
            v.push(format!("{} service(s) now start only when needed", self.services));
        }
        if self.cancelled_admin > 0 {
            v.push(format!("{} change(s) skipped: admin approval was cancelled", self.cancelled_admin));
        }
        if !self.failed.is_empty() {
            v.push(format!("{} change(s) didn't work: {}", self.failed.len(), self.failed[0]));
        }
        if v.is_empty() {
            v.push("Nothing needed changing.".into());
        }
        v
    }
}

fn is_dry_run() -> bool {
    std::env::var_os("OCO_DRY_RUN").is_some()
}

/// Runs `ops` in three groups (closing programs, junk, everything else) so the
/// UI can show progress; admin-only work is collected and done in one elevated
/// batch at the end, so Windows asks at most once.
pub fn run(ops: Vec<Op>, junk_estimate: u64, progress: &dyn Fn(usize)) -> RunSummary {
    let mem_before = sys::mem::read_mem().commit_used;
    let mut sum = RunSummary { mem_before, dry_run: is_dry_run(), ..Default::default() };
    let groups: [Vec<Op>; 3] = {
        let (mut kills, mut junk, mut rest) = (Vec::new(), Vec::new(), Vec::new());
        for op in ops {
            match &op {
                Op::Apply { action: Action::KillProcesses { .. } } => kills.push(op),
                Op::Apply { action: Action::CleanJunk { .. } } => junk.push(op),
                _ => rest.push(op),
            }
        }
        [kills, junk, rest]
    };

    if sum.dry_run {
        for (i, g) in groups.iter().enumerate() {
            progress(i);
            std::thread::sleep(std::time::Duration::from_millis(500));
            for op in g {
                if let Op::Apply { action } = op {
                    count(&mut sum, action, true, 0);
                }
            }
        }
        sum.junk_freed = if groups[1].is_empty() { 0 } else { junk_estimate };
        progress(3);
        sum.mem_after = mem_before;
        return sum;
    }

    let path = journal_path();
    let before = Journal::load(&path).entries.len();
    let mut deferred = Vec::new();
    for (i, g) in groups.into_iter().enumerate() {
        progress(i);
        deferred.extend(actions::execute(&path, g, false));
    }
    if !deferred.is_empty() {
        let n = deferred.len();
        match elevate::run_elevated(&Plan { journal: path.clone(), ops: deferred }) {
            Ok(_) => {}
            Err(ElevateError::Cancelled) => sum.cancelled_admin = n,
            Err(ElevateError::Failed(e)) => sum.failed.push(e),
        }
    }
    progress(3);
    let after = Journal::load(&path).entries;
    for e in &after[before.min(after.len())..] {
        if e.ok {
            count(&mut sum, &e.action, true, e.freed_bytes);
        } else {
            sum.failed.push(e.message.clone());
        }
    }
    // Give Windows a moment to release the memory before measuring.
    std::thread::sleep(std::time::Duration::from_millis(1500));
    sum.mem_after = sys::mem::read_mem().commit_used;
    sum
}

fn count(sum: &mut RunSummary, action: &Action, ok: bool, freed: u64) {
    if !ok {
        return;
    }
    sum.applied += 1;
    match action {
        Action::KillProcesses { .. } => sum.closed += 1,
        Action::CleanJunk { .. } | Action::DeleteDevFolder { .. } => sum.junk_freed += freed,
        Action::SetStartupEnabled { enabled: false, .. } => sum.startup_off += 1,
        Action::SetStartupEnabled { .. } => {}
        Action::SetServiceStart { .. } => sum.services += 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::ProcTarget;
    use crate::model::{MemInfo, RunLocation};

    fn result() -> ScanResult {
        let kill = Action::KillProcesses { label: "Gradle daemon".into(), targets: vec![ProcTarget { pid: 1, name: "java.exe".into(), created: 1 }] };
        let rec = |category, action: Action, saves, preselect| Recommendation { category, title: "t".into(), why: "w".into(), saves_bytes: saves, action, preselect };
        let off = Action::SetStartupEnabled { location: RunLocation::HkcuRun, value_name: "MuMu".into(), enabled: false };
        let jf = |id: &str, bytes, default_on| Found { id: id.into(), name: id.into(), bytes, default_on, measurable: true, ..Default::default() };
        ScanResult {
            snap: Snapshot { mem: MemInfo { total_phys: 8 << 30, avail_phys: 1 << 30, commit_used: 16 << 30, commit_limit: 28 << 30 }, ..Default::default() },
            recs: vec![
                rec(Category::DevTool, kill, 1500 << 20, true),
                rec(Category::DevTool, Action::KillProcesses { label: "Vite".into(), targets: vec![] }, 300 << 20, false),
                rec(Category::Startup, off, 0, true),
            ],
            junk: vec![jf("user_temp", 700 << 20, true), jf("crash_dumps", 0, true), jf("browser_cache", 900 << 20, false)],
            insights: vec![],
            facts: Facts::default(),
            score: 40,
            score_after: 80,
        }
    }

    #[test]
    fn default_plan_is_preticked_fixes_plus_nonempty_default_junk() {
        let r = result();
        let ops = default_ops(&r);
        assert_eq!(ops.len(), 3, "{ops:#?}");
        assert!(ops.iter().any(|o| matches!(o, Op::Apply { action: Action::CleanJunk { id, .. } } if id == "user_temp")));
        assert!(!ops.iter().any(|o| matches!(o, Op::Apply { action: Action::CleanJunk { id, .. } } if id == "browser_cache")));
        let lines = plan_lines(&r, &ops);
        assert_eq!(lines, vec![
            "Close 1 idle program(s) \u{2014} frees 1.5 GB".to_string(),
            "Delete 700 MB of junk files".to_string(),
            "Stop 1 app(s) opening at sign-in".to_string(),
        ]);
        assert_eq!(r.fix_bytes(), 1500 << 20);
        assert_eq!(r.junk_bytes(), 700 << 20);
    }

    #[test]
    fn kept_programs_are_never_ticked() {
        assert_eq!(keep_key("2 Gradle daemons"), keep_key("Gradle daemon"));
        assert_eq!(keep_key("3 Android Debug Bridge (adb) servers"), "android debug bridge (adb) server");
        assert_eq!(keep_key("Vite dev server (shop)"), "vite dev server (shop)");
        let mut r = result();
        apply_keep(&mut r.recs, &["gradle daemon".to_string()]);
        assert!(!r.recs[0].preselect && r.recs[0].why.contains("keep it running"));
        assert!(r.recs[2].preselect, "other fixes are untouched");
    }

    #[test]
    fn summary_lines() {
        let s = RunSummary { mem_before: 16 << 30, mem_after: 14 << 30, closed: 2, junk_freed: 700 << 20, startup_off: 1, ..Default::default() };
        assert_eq!(s.lines()[0], "Freed 2.0 GB of memory");
        assert_eq!(s.lines().len(), 4);
        assert_eq!(RunSummary::default().lines(), vec!["Nothing needed changing.".to_string()]);
    }
}
