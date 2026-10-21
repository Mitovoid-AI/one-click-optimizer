//! Turns a snapshot into a pressure reading, per-app totals and fix
//! recommendations. Pure functions, no Windows calls.

use crate::actions::{Action, ProcTarget};
use crate::kb::{self, ProcKind};
use crate::model::*;
use crate::util::{fmt_duration, FT_PER_SEC};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Level {
    Normal,
    High,
    Critical,
}

impl Level {
    pub fn label(self) -> &'static str {
        match self {
            Level::Normal => "Normal",
            Level::High => "High",
            Level::Critical => "Critical",
        }
    }
}

/// (committed memory / physical RAM, level).
pub fn pressure(m: &MemInfo) -> (f64, Level) {
    if m.total_phys == 0 {
        return (0.0, Level::Normal);
    }
    let ratio = m.commit_used as f64 / m.total_phys as f64;
    let free = m.avail_phys as f64 / m.total_phys as f64;
    let level = if ratio >= 1.5 || free < 0.05 {
        Level::Critical
    } else if ratio >= 1.0 || free < 0.12 {
        Level::High
    } else {
        Level::Normal
    };
    (ratio, level)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Category {
    DevTool,
    BackgroundHelper,
    Service,
    Startup,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Category::DevTool => "Idle dev tool",
            Category::BackgroundHelper => "Background helper",
            Category::Service => "Service",
            Category::Startup => "Startup app",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Recommendation {
    pub category: Category,
    pub title: String,
    pub why: String,
    /// Memory freed right now (0 for changes that only help at next boot).
    pub saves_bytes: u64,
    pub action: Action,
    pub preselect: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct AppGroup {
    pub name: String,
    pub label: String,
    pub count: usize,
    pub private_bytes: u64,
    pub working_set: u64,
    /// Processes that may be ended (never protected ones or the app itself).
    pub targets: Vec<ProcTarget>,
    pub protected: bool,
}

fn display_name(exe: &str) -> String {
    let lower = exe.to_ascii_lowercase();
    match lower.strip_suffix(".exe") {
        Some(_) => exe[..exe.len() - 4].to_string(),
        None => exe.to_string(),
    }
}

fn target(p: &ProcInfo) -> ProcTarget {
    ProcTarget { pid: p.pid, name: p.name.clone(), created: p.created }
}

fn age_secs(now: u64, p: &ProcInfo) -> u64 {
    if p.created == 0 {
        0
    } else {
        now.saturating_sub(p.created) / FT_PER_SEC
    }
}

pub fn group_apps(procs: &[ProcInfo], self_pid: u32) -> Vec<AppGroup> {
    let mut map: BTreeMap<String, AppGroup> = BTreeMap::new();
    for p in procs.iter().filter(|p| p.pid != 0) {
        let key = p.name.to_ascii_lowercase();
        let g = map.entry(key).or_insert_with(|| AppGroup {
            name: display_name(&p.name),
            label: kb::app_label(&p.name).to_string(),
            count: 0,
            private_bytes: 0,
            working_set: 0,
            targets: Vec::new(),
            protected: kb::is_protected_process(&p.name),
        });
        g.count += 1;
        g.private_bytes += p.private_bytes;
        g.working_set += p.working_set;
        if p.pid != self_pid && !g.protected {
            g.targets.push(target(p));
        }
    }
    let mut v: Vec<AppGroup> = map.into_values().collect();
    v.sort_by(|a, b| b.private_bytes.cmp(&a.private_bytes));
    v
}

const DEV_SERVER_MIN_AGE: u64 = 30 * 60;
const DAEMON_MIN_AGE: u64 = 10 * 60;
const IDLE_CPU_PCT: f32 = 1.0;

pub fn analyze(s: &Snapshot) -> Vec<Recommendation> {
    let mut recs = Vec::new();
    dev_tools(s, &mut recs);
    helpers(s, &mut recs);
    services(s, &mut recs);
    startup(s, &mut recs);
    recs.sort_by(|a, b| b.saves_bytes.cmp(&a.saves_bytes));
    recs
}

fn ports_text(ports: &[u16]) -> String {
    match ports.len() {
        0 => String::new(),
        1 => format!(" (port {})", ports[0]),
        _ => format!(
            " (ports {})",
            ports.iter().map(u16::to_string).collect::<Vec<_>>().join(", ")
        ),
    }
}

fn dev_tools(s: &Snapshot, recs: &mut Vec<Recommendation>) {
    let by_pid: HashMap<u32, &ProcInfo> = s.procs.iter().map(|p| (p.pid, p)).collect();

    struct Inst<'a> {
        p: &'a ProcInfo,
        tool: &'static str,
        project: String,
        wrapper: Option<&'a ProcInfo>,
    }
    let mut servers: Vec<Inst> = Vec::new();
    let mut daemons: [Vec<(&ProcInfo, u64, bool)>; 3] = [Vec::new(), Vec::new(), Vec::new()];

    for p in &s.procs {
        if p.pid == s.self_pid {
            continue;
        }
        let age = age_secs(s.now, p);
        let idle = p.cpu_pct < IDLE_CPU_PCT;
        match kb::classify_process(&p.name, &p.cmdline) {
            Some(ProcKind::DevServer(tool)) => servers.push(Inst {
                p,
                tool,
                project: kb::project_from_cmdline(&p.cmdline).unwrap_or_else(|| "unknown project".into()),
                wrapper: by_pid
                    .get(&p.ppid)
                    .copied()
                    .filter(|pp| kb::is_npm_wrapper(&pp.name, &pp.cmdline)),
            }),
            Some(kind @ (ProcKind::GradleDaemon | ProcKind::KotlinDaemon | ProcKind::Adb)) => {
                if age >= DAEMON_MIN_AGE {
                    let slot = match kind {
                        ProcKind::GradleDaemon => 0,
                        ProcKind::KotlinDaemon => 1,
                        _ => 2,
                    };
                    daemons[slot].push((p, age, idle));
                }
            }
            _ => {}
        }
    }

    // One row per kind of build daemon, however many copies are running.
    const DAEMONS: [(&str, &str, &str); 3] = [
        ("Gradle daemon", "Gradle daemons", "Android/Java build helper kept in memory between builds. The next build starts a fresh one."),
        ("Kotlin compile daemon", "Kotlin compile daemons", "Kotlin compiler kept in memory between builds. The next build starts a fresh one."),
        ("Android Debug Bridge (adb) server", "Android Debug Bridge (adb) servers", "Starts again automatically when you use adb or Android Studio."),
    ];
    for (found, (one, many, what)) in daemons.iter().zip(DAEMONS) {
        if found.is_empty() {
            continue;
        }
        let n = found.len();
        let label = if n == 1 { one.to_string() } else { format!("{n} {many}") };
        let oldest = found.iter().map(|f| f.1).max().unwrap_or(0);
        let idle = found.iter().all(|f| f.2);
        recs.push(Recommendation {
            category: Category::DevTool,
            title: format!("Stop {}{label}", if n == 1 { "the " } else { "" }),
            why: format!(
                "Running for {}, {}. {what}",
                fmt_duration(oldest),
                if idle { "using no CPU" } else { "still using some CPU (may be in use)" }
            ),
            saves_bytes: found.iter().map(|f| f.0.private_bytes).sum(),
            action: Action::KillProcesses { label, targets: found.iter().map(|f| target(f.0)).collect() },
            preselect: idle,
        });
    }

    let mut per_project: HashMap<&str, usize> = HashMap::new();
    for i in &servers {
        *per_project.entry(i.project.as_str()).or_default() += 1;
    }
    for i in &servers {
        let copies = per_project[i.project.as_str()];
        let age = age_secs(s.now, i.p);
        if age < DEV_SERVER_MIN_AGE && copies < 2 {
            continue; // fresh and alone: probably in use right now
        }
        let cpu = i.p.cpu_pct + i.wrapper.map_or(0.0, |w| w.cpu_pct);
        let idle = cpu < IDLE_CPU_PCT;
        let mut ports = i.p.ports.clone();
        let mut targets = vec![target(i.p)];
        let mut bytes = i.p.private_bytes;
        // Helpers the server spawned (esbuild, workers) go with it.
        for c in s.procs.iter().filter(|c| c.ppid == i.p.pid && c.created >= i.p.created) {
            targets.push(target(c));
            bytes += c.private_bytes;
        }
        if let Some(w) = i.wrapper {
            ports.extend(&w.ports);
            targets.push(target(w));
            bytes += w.private_bytes;
        }
        ports.sort_unstable();
        ports.dedup();
        let mut why = format!(
            "Running for {}, {}.",
            fmt_duration(age),
            if idle { "using no CPU".to_string() } else { format!("still using {cpu:.1}% CPU (may be in use)") }
        );
        if copies > 1 {
            why.push_str(&format!(" {copies} copies of this project's dev server are running."));
        }
        why.push_str(" Start it again with your dev command (for example npm run dev) when you need it.");
        recs.push(Recommendation {
            category: Category::DevTool,
            title: format!("Stop {} dev server: {}{}", i.tool, i.project, ports_text(&ports)),
            why,
            saves_bytes: bytes,
            action: Action::KillProcesses { label: format!("{} dev server ({})", i.tool, i.project), targets },
            preselect: idle && age >= DEV_SERVER_MIN_AGE,
        });
    }
}

fn helpers(s: &Snapshot, recs: &mut Vec<Recommendation>) {
    let mut groups: BTreeMap<&'static str, (&'static str, Vec<ProcTarget>, u64)> = BTreeMap::new();
    for p in s.procs.iter().filter(|p| p.pid != s.self_pid) {
        if let Some(ProcKind::Helper { label, why }) = kb::classify_process(&p.name, &p.cmdline) {
            let g = groups.entry(label).or_insert((why, Vec::new(), 0));
            g.1.push(target(p));
            g.2 += p.private_bytes;
        }
    }
    for (label, (why, targets, bytes)) in groups {
        recs.push(Recommendation {
            category: Category::BackgroundHelper,
            title: format!("Close {label}"),
            why: why.to_string(),
            saves_bytes: bytes,
            action: Action::KillProcesses { label: label.to_string(), targets },
            preselect: true,
        });
    }
}

fn services(s: &Snapshot, recs: &mut Vec<Recommendation>) {
    for svc in &s.services {
        if kb::is_protected_service(&svc.name) || svc.start_type != SERVICE_AUTO_START {
            continue;
        }
        let Some(why) = kb::service_advice(&svc.name) else { continue };
        let saves = if svc.running && svc.pid != 0 {
            s.procs
                .iter()
                .find(|p| p.pid == svc.pid && !p.name.eq_ignore_ascii_case("svchost.exe"))
                .map_or(0, |p| p.private_bytes)
        } else {
            0
        };
        recs.push(Recommendation {
            category: Category::Service,
            title: format!("Start \"{}\" only when needed", svc.display),
            why: why.to_string(),
            saves_bytes: saves,
            action: Action::SetServiceStart {
                name: svc.name.clone(),
                display: svc.display.clone(),
                start: SERVICE_DEMAND_START,
                stop: svc.running,
            },
            preselect: true,
        });
    }
}

/// `"C:\Program Files\App\app.exe" --flag` -> `c:\program files\app\app.exe`.
pub fn exe_from_command(cmd: &str) -> String {
    let c = cmd.trim();
    let path = if let Some(rest) = c.strip_prefix('"') {
        rest.split('"').next().unwrap_or("")
    } else {
        let lower = c.to_ascii_lowercase();
        match lower.find(".exe") {
            Some(i) => &c[..i + 4],
            None => c.split_whitespace().next().unwrap_or(""),
        }
    };
    path.to_ascii_lowercase()
}

fn startup(s: &Snapshot, recs: &mut Vec<Recommendation>) {
    for e in s.startup.iter().filter(|e| e.enabled) {
        if kb::is_protected_startup(&e.name) {
            continue;
        }
        let Some(why) = kb::startup_advice(&e.name) else { continue };
        let exe = exe_from_command(&e.command);
        let running: u64 = s
            .procs
            .iter()
            .filter(|p| !exe.is_empty() && p.path.eq_ignore_ascii_case(&exe))
            .map(|p| p.private_bytes)
            .sum();
        recs.push(Recommendation {
            category: Category::Startup,
            title: format!("Don't start \"{}\" with Windows", e.name),
            why: why.to_string(),
            saves_bytes: running,
            action: Action::SetStartupEnabled { location: e.location, value_name: e.name.clone(), enabled: false },
            preselect: true,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: u64 = 3600 * FT_PER_SEC;
    const NOW: u64 = 1_000 * HOUR;

    fn proc(pid: u32, ppid: u32, name: &str, cmd: &str, mb: u64, age_h: f64) -> ProcInfo {
        ProcInfo {
            pid,
            ppid,
            name: name.into(),
            cmdline: cmd.into(),
            private_bytes: mb << 20,
            working_set: mb << 20,
            created: NOW - (age_h * HOUR as f64) as u64,
            ..Default::default()
        }
    }

    fn vite(pid: u32, ppid: u32, port: u16) -> ProcInfo {
        let mut p = proc(pid, ppid, "node.exe", r#""node" "Z:\code\my-shop\node_modules\.bin\\..\vite\bin\vite.js" --port 5300"#, 300, 3.0);
        p.ports = vec![port];
        p
    }

    fn npx(pid: u32) -> ProcInfo {
        proc(pid, 1, "node.exe", r"node C:\npm\node_modules\npm\bin\npx-cli.js vite --port 5300", 30, 3.0)
    }

    fn snap(procs: Vec<ProcInfo>) -> Snapshot {
        Snapshot { procs, now: NOW, self_pid: 99_999, ..Default::default() }
    }

    #[test]
    fn duplicate_idle_vite_servers_include_npx_wrappers() {
        let s = snap(vec![npx(10), vite(11, 10, 5300), npx(20), vite(21, 20, 5301), npx(30), vite(31, 30, 5330)]);
        let recs = analyze(&s);
        assert_eq!(recs.len(), 3, "{recs:#?}");
        for r in &recs {
            assert_eq!(r.category, Category::DevTool);
            assert!(r.preselect);
            assert!(r.why.contains("3 copies"), "{}", r.why);
            assert_eq!(r.saves_bytes, 330 << 20);
            match &r.action {
                Action::KillProcesses { targets, .. } => assert_eq!(targets.len(), 2),
                a => panic!("unexpected {a:?}"),
            }
        }
        assert!(recs.iter().any(|r| r.title.contains("my-shop (port 5330)")));
    }

    #[test]
    fn dev_server_children_are_included() {
        let mut esb = proc(12, 11, "esbuild.exe", "esbuild --service=0.21.5 --ping", 50, 2.9);
        esb.created = vite(11, 10, 5300).created + 1;
        let recs = analyze(&snap(vec![npx(10), vite(11, 10, 5300), esb]));
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].saves_bytes, 380 << 20);
        match &recs[0].action {
            Action::KillProcesses { targets, .. } => {
                assert_eq!(targets.iter().map(|t| t.pid).collect::<Vec<_>>(), vec![11, 12, 10])
            }
            a => panic!("unexpected {a:?}"),
        }
    }

    #[test]
    fn fresh_single_dev_server_is_left_alone() {
        let mut v = vite(11, 10, 5300);
        v.created = NOW - HOUR / 12; // 5 minutes
        assert!(analyze(&snap(vec![npx(10), v])).is_empty());
    }

    #[test]
    fn busy_dev_server_is_not_preselected() {
        let mut v = vite(11, 10, 5300);
        v.cpu_pct = 12.0;
        let recs = analyze(&snap(vec![v]));
        assert_eq!(recs.len(), 1);
        assert!(!recs[0].preselect);
        assert!(!recs[0].why.contains("no CPU"));
    }

    #[test]
    fn gradle_daemon_needs_age() {
        let cmd = "java org.gradle.launcher.daemon.bootstrap.GradleDaemon 8.2.1";
        let old = proc(5, 1, "java.exe", cmd, 1073, 2.0);
        let young = proc(6, 1, "java.exe", cmd, 1073, 0.05);
        let recs = analyze(&snap(vec![old, young]));
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].saves_bytes, 1073 << 20);
        assert!(recs[0].title.contains("Gradle"));
    }

    #[test]
    fn same_kind_daemons_share_one_row() {
        let adb = |pid| proc(pid, 1, "adb.exe", "adb -L tcp:5037 fork-server server", 2, 1.0);
        let recs = analyze(&snap(vec![adb(7), adb(8)]));
        assert_eq!(recs.len(), 1, "{recs:#?}");
        assert_eq!(recs[0].title, "Stop 2 Android Debug Bridge (adb) servers");
        assert_eq!(recs[0].saves_bytes, 4 << 20);
        match &recs[0].action {
            Action::KillProcesses { targets, .. } => assert_eq!(targets.len(), 2),
            a => panic!("unexpected {a:?}"),
        }
    }

    #[test]
    fn helpers_are_grouped() {
        let s = snap(vec![
            proc(1, 0, "LenovoVantage-(GenericTelemetryAddin).exe", "", 38, 20.0),
            proc(2, 0, "LenovoVantage-(VantageCoreAddin).exe", "", 33, 20.0),
            proc(3, 0, "XboxPcTray.exe", "", 35, 20.0),
        ]);
        let recs = analyze(&s);
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].title, "Close Lenovo Vantage add-ins");
        assert_eq!(recs[0].saves_bytes, 71 << 20);
    }

    #[test]
    fn services_only_auto_known_unprotected() {
        let mut s = snap(vec![proc(77, 0, "AGMService.exe", "", 40, 20.0)]);
        let svc = |name: &str, start: u32, pid: u32| ServiceInfo {
            name: name.into(),
            display: name.into(),
            running: pid != 0,
            start_type: start,
            path: String::new(),
            pid,
        };
        s.services = vec![
            svc("AGMService", SERVICE_AUTO_START, 77),
            svc("UDCService", SERVICE_DEMAND_START, 0),
            svc("LenovoFnAndFunctionKeys", SERVICE_AUTO_START, 0),
            svc("Spooler", SERVICE_AUTO_START, 0),
        ];
        let recs: Vec<_> = analyze(&s).into_iter().filter(|r| r.category == Category::Service).collect();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].saves_bytes, 40 << 20);
        assert_eq!(
            recs[0].action,
            Action::SetServiceStart { name: "AGMService".into(), display: "AGMService".into(), start: SERVICE_DEMAND_START, stop: true }
        );
    }

    #[test]
    fn startup_entries() {
        let mut fdm = proc(50, 0, "fdm.exe", "", 60, 20.0);
        fdm.path = r"C:\Program Files\Softdeluxe\Free Download Manager\fdm.exe".into();
        let mut s = snap(vec![fdm]);
        let e = |name: &str, cmd: &str, enabled: bool| StartupEntry {
            location: RunLocation::HkcuRun,
            name: name.into(),
            command: cmd.into(),
            enabled,
        };
        s.startup = vec![
            e("Free Download Manager", r#""C:\Program Files\Softdeluxe\Free Download Manager\fdm.exe" --hidden"#, true),
            e("MuMuPlayerGlobal", r"C:\MuMu\MuMu.exe", false),
            e("SecurityHealth", r"%windir%\system32\SecurityHealthSystray.exe", true),
        ];
        let recs = analyze(&s);
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].saves_bytes, 60 << 20);
    }

    #[test]
    fn exe_parsing() {
        assert_eq!(exe_from_command(r#""C:\A B\x.exe" -y"#), r"c:\a b\x.exe");
        assert_eq!(exe_from_command(r"C:\Prog Files\y.EXE /min"), r"c:\prog files\y.exe");
        assert_eq!(exe_from_command(""), "");
    }

    #[test]
    fn pressure_levels() {
        let gb = 1u64 << 30;
        let m = |commit: u64, avail: u64| MemInfo { total_phys: 8 * gb, avail_phys: avail, commit_used: commit, commit_limit: 28 * gb };
        assert_eq!(pressure(&m(5 * gb, 3 * gb)).1, Level::Normal);
        assert_eq!(pressure(&m(11 * gb, 2 * gb)).1, Level::High);
        assert_eq!(pressure(&m(12 * gb, 2 * gb)).1, Level::Critical); // exactly 150%
        assert_eq!(pressure(&m(16 * gb, 2 * gb)).1, Level::Critical);
        assert_eq!(pressure(&m(5 * gb, gb / 4)).1, Level::Critical);
        assert_eq!(pressure(&MemInfo::default()).1, Level::Normal);
    }

    #[test]
    fn app_groups() {
        let s = vec![
            proc(1, 0, "chrome.exe", "", 200, 1.0),
            proc(2, 0, "chrome.exe", "", 100, 1.0),
            proc(3, 0, "svchost.exe", "", 50, 1.0),
            proc(4, 0, "oneclickoptimizer.exe", "", 5, 1.0),
        ];
        let g = group_apps(&s, 4);
        assert_eq!(g[0].name, "chrome");
        assert_eq!(g[0].count, 2);
        assert_eq!(g[0].targets.len(), 2);
        assert_eq!(g[0].label, "Google Chrome");
        let svc = g.iter().find(|x| x.name == "svchost").unwrap();
        assert!(svc.protected && svc.targets.is_empty());
        let me = g.iter().find(|x| x.name == "oneclickoptimizer").unwrap();
        assert!(me.targets.is_empty());
    }
}
