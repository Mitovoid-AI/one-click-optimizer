//! Deep-scan health checks (things that slow a PC but aren't a single
//! process to close) and the 0-100 health score.

use crate::analyze::{group_apps, pressure, Category, Recommendation};
use crate::model::{MemInfo, Snapshot};
use crate::sys::hw::RamInfo;
use crate::util::{fmt_bytes, fmt_duration};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Severity {
    Tip,
    Warning,
    Problem,
}

#[derive(Clone, Debug, Serialize)]
pub struct Insight {
    pub severity: Severity,
    pub title: String,
    pub detail: String,
}

/// System facts gathered during the deep scan.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Facts {
    pub uptime_secs: u64,
    /// (free, total) bytes on C:.
    pub disk_c: Option<(u64, u64)>,
    pub ram: Option<RamInfo>,
}

const GB: u64 = 1 << 30;
const MB: u64 = 1 << 20;

fn gb(mb: u64) -> String {
    format!("{} GB", (mb as f64 / 1024.0).round() as u64)
}

pub fn insights(s: &Snapshot, f: &Facts) -> Vec<Insight> {
    let mut v = Vec::new();
    let (ratio, _) = pressure(&s.mem);

    if f.uptime_secs >= 3 * 86_400 {
        v.push(Insight {
            severity: Severity::Warning,
            title: "Restart your PC".into(),
            detail: format!(
                "It has been running for {} without a restart. Restarting clears leftover programs and memory leaks.",
                fmt_duration(f.uptime_secs)
            ),
        });
    }

    if let Some((free, total)) = f.disk_c {
        if total > 0 && (free < 10 * GB || free * 10 < total) {
            v.push(Insight {
                severity: if free < 5 * GB { Severity::Problem } else { Severity::Warning },
                title: "Low disk space on C:".into(),
                detail: format!(
                    "Only {} of {} is free. Windows uses free space on C: as extra memory, so a full drive makes everything slower.",
                    fmt_bytes(free),
                    fmt_bytes(total)
                ),
            });
        }
    }

    let page_file = s.mem.commit_limit.saturating_sub(s.mem.total_phys);
    if s.mem.total_phys > 0 && page_file < 512 * MB {
        v.push(Insight {
            severity: Severity::Problem,
            title: "Virtual memory is turned off or tiny".into(),
            detail: "Windows has almost no page file, so apps can crash when memory runs out. Set it back to \u{201C}System managed\u{201D} in Advanced system settings.".into(),
        });
    }

    for p in s.procs.iter().filter(|p| p.pid > 4 && p.handles > 20_000) {
        v.push(Insight {
            severity: Severity::Warning,
            title: format!("{} may be leaking", p.name.trim_end_matches(".exe")),
            detail: format!(
                "It holds {} open handles (normal is a few thousand). Closing and reopening it usually frees the memory.",
                p.handles
            ),
        });
    }

    for p in s.procs.iter().filter(|p| p.growth_bytes > 150 * MB as i64) {
        v.push(Insight {
            severity: Severity::Tip,
            title: format!("{} is growing fast", p.name.trim_end_matches(".exe")),
            detail: format!(
                "It took {} more memory during the scan. If it isn't loading something big, it may be leaking.",
                fmt_bytes(p.growth_bytes as u64)
            ),
        });
    }

    let groups = group_apps(&s.procs, s.self_pid);
    for g in &groups {
        let n = g.name.to_ascii_lowercase();
        let shell = matches!(n.as_str(), "powershell" | "pwsh" | "cmd" | "bash" | "python" | "pythonw");
        if shell && g.count >= 8 && g.private_bytes >= 300 * MB {
            v.push(Insight {
                severity: Severity::Tip,
                title: format!("{} {} windows are open", g.count, g.name),
                detail: format!(
                    "Together they use {}. Close the terminals and scripts you no longer need.",
                    fmt_bytes(g.private_bytes)
                ),
            });
        }
    }
    let browser: u64 = groups
        .iter()
        .filter(|g| matches!(g.name.to_ascii_lowercase().as_str(), "chrome" | "msedge" | "brave" | "firefox" | "opera"))
        .map(|g| g.private_bytes)
        .sum();
    if browser >= 3 * GB / 2 {
        v.push(Insight {
            severity: Severity::Tip,
            title: format!("Your browser uses {}", fmt_bytes(browser)),
            detail: "Turn on Memory Saver (Settings \u{2192} Performance) so tabs you aren't using go to sleep.".into(),
        });
    }

    if s.mem.total_phys > 0 && s.mem.total_phys <= 17 * GB && ratio >= 1.3 {
        let ram_gb = (s.mem.total_phys as f64 / GB as f64).round() as u64;
        let detail = match &f.ram {
            Some(r) => upgrade_advice(r),
            None => format!("Programs regularly ask for more than your {ram_gb} GB. Adding RAM is the fix that helps most."),
        };
        v.push(Insight { severity: Severity::Tip, title: format!("{ram_gb} GB of RAM is your bottleneck"), detail });
    }
    v
}

/// Plain-language upgrade advice from the installed modules.
pub fn upgrade_advice(r: &RamInfo) -> String {
    let total = r.total_mb();
    let soldered = r.soldered_mb();
    let removable = r.removable();
    let layout = if soldered > 0 && !removable.is_empty() {
        format!("{} built in + {} in a slot", gb(soldered), removable.iter().map(|m| gb(m.size_mb)).collect::<Vec<_>>().join(" + "))
    } else if soldered == total {
        "all built into the board".to_string()
    } else {
        format!("{} stick(s)", removable.len())
    };
    let kind = r.modules.first().map(|m| format!("{} {}", m.kind, if m.speed_mts > 0 { m.speed_mts.to_string() } else { String::new() }))
        .unwrap_or_default();
    let mut s = format!("You have {} ({layout}).", gb(total));
    if r.empty_slots > 0 {
        s.push_str(&format!(" There {} {} empty slot(s): adding a 16 GB {} stick is the biggest speed-up you can get.", if r.empty_slots == 1 { "is" } else { "are" }, r.empty_slots, kind.trim()));
    } else if let Some(small) = removable.iter().min_by_key(|m| m.size_mb) {
        s.push_str(&format!(
            " Swapping the {} stick for a 16 GB {} stick would give you {}.",
            gb(small.size_mb),
            kind.trim(),
            gb(total - small.size_mb + 16 * 1024)
        ));
    } else {
        s.push_str(" The RAM is soldered, so it can't be upgraded: closing apps is the only fix.");
    }
    s
}

/// 0-100: higher is healthier.
pub fn score(mem: &MemInfo, recs: &[Recommendation], junk_bytes: u64, ins: &[Insight]) -> u8 {
    let (ratio, _) = pressure(mem);
    let fix_bytes: u64 = recs.iter().filter(|r| r.preselect).map(|r| r.saves_bytes).sum();
    let mut s: i32 = 100;
    s -= if ratio >= 1.5 { 30 } else if ratio >= 1.0 { 15 } else { 0 };
    s -= match fix_bytes {
        b if b >= GB => 15,
        b if b >= 300 * MB => 8,
        b if b > 0 => 4,
        _ => 0,
    };
    let startup_like = recs.iter().filter(|r| matches!(r.category, Category::Startup | Category::Service)).count() as i32;
    s -= (startup_like * 3).min(12);
    s -= match junk_bytes {
        b if b >= 5 * GB => 15,
        b if b >= GB => 10,
        b if b >= 200 * MB => 5,
        _ => 0,
    };
    s -= ins
        .iter()
        .map(|i| match i.severity {
            Severity::Problem => 8,
            Severity::Warning => 5,
            Severity::Tip => 0,
        })
        .sum::<i32>()
        .min(20);
    s.clamp(5, 100) as u8
}

/// Estimated score once the selected memory fixes and junk cleaning are done.
pub fn score_after(mem: &MemInfo, freed: u64, ins: &[Insight]) -> u8 {
    let after = MemInfo { commit_used: mem.commit_used.saturating_sub(freed), ..mem.clone() };
    score(&after, &[], 0, ins)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ProcInfo;
    use crate::sys::hw::RamModule;

    fn mem(commit_gb: u64) -> MemInfo {
        MemInfo { total_phys: 8 * GB, avail_phys: GB, commit_used: commit_gb * GB, commit_limit: 28 * GB }
    }

    fn laptop() -> RamInfo {
        RamInfo {
            modules: vec![
                RamModule { locator: "DIMM0".into(), size_mb: 4096, speed_mts: 3200, kind: "DDR4".into(), form: "SO-DIMM".into(), soldered: false, part: String::new() },
                RamModule { locator: "ChannelB".into(), size_mb: 4096, speed_mts: 3200, kind: "DDR4".into(), form: "SO-DIMM".into(), soldered: true, part: String::new() },
            ],
            empty_slots: 0,
            max_capacity_mb: 32768,
        }
    }

    #[test]
    fn upgrade_advice_for_half_soldered_laptop() {
        let a = upgrade_advice(&laptop());
        assert!(a.contains("8 GB (4 GB built in + 4 GB in a slot)"), "{a}");
        assert!(a.contains("Swapping the 4 GB stick for a 16 GB DDR4 3200 stick would give you 20 GB"), "{a}");
    }

    #[test]
    fn insights_from_snapshot() {
        let mut s = Snapshot { mem: mem(16), self_pid: 1, ..Default::default() };
        s.procs = (0..10).map(|i| ProcInfo { pid: 100 + i, name: "powershell.exe".into(), private_bytes: 70 * MB, ..Default::default() }).collect();
        // Many small shells are not worth a tip.
        s.procs.extend((0..9).map(|i| ProcInfo { pid: 200 + i, name: "bash.exe".into(), private_bytes: 4 * MB, ..Default::default() }));
        s.procs.push(ProcInfo { pid: 9, name: "leaky.exe".into(), handles: 50_000, growth_bytes: 400 * MB as i64, ..Default::default() });
        let f = Facts { uptime_secs: 5 * 86_400, disk_c: Some((3 * GB, 250 * GB)), ram: Some(laptop()) };
        let ins = insights(&s, &f);
        let titles: Vec<&str> = ins.iter().map(|i| i.title.as_str()).collect();
        assert!(titles.contains(&"Restart your PC"), "{titles:?}");
        assert!(titles.contains(&"Low disk space on C:"));
        assert!(titles.contains(&"leaky may be leaking"));
        assert!(titles.contains(&"leaky is growing fast"));
        assert!(titles.contains(&"10 powershell windows are open"));
        assert!(!titles.iter().any(|t| t.contains("bash")), "{titles:?}");
        assert!(titles.contains(&"8 GB of RAM is your bottleneck"));
        assert!(!titles.iter().any(|t| t.contains("Virtual memory")));
        // Healthy machine: nothing to say.
        let calm = Snapshot { mem: MemInfo { total_phys: 16 * GB, avail_phys: 8 * GB, commit_used: 8 * GB, commit_limit: 40 * GB }, ..Default::default() };
        assert!(insights(&calm, &Facts { uptime_secs: 3600, disk_c: Some((200 * GB, 500 * GB)), ram: None }).is_empty());
    }

    #[test]
    fn score_moves_the_right_way() {
        let bad = score(&mem(16), &[], 3 * GB, &[Insight { severity: Severity::Warning, title: String::new(), detail: String::new() }]);
        let good = score(&MemInfo { total_phys: 16 * GB, avail_phys: 8 * GB, commit_used: 8 * GB, commit_limit: 40 * GB }, &[], 0, &[]);
        assert_eq!(good, 100);
        assert!(bad < 60, "{bad}");
        assert!(score_after(&mem(16), 6 * GB, &[]) > score(&mem(16), &[], 0, &[]));
    }
}
