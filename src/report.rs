//! Plain-text and JSON reports (used by oneclick-cli).

use crate::analyze::{group_apps, pressure, AppGroup, Level, Recommendation};
use crate::health::{Facts, Insight};
use crate::junk::Found;
use crate::model::MemInfo;
use crate::optimize::ScanResult;
use crate::util::fmt_bytes;
use serde::Serialize;

#[derive(Serialize)]
pub struct Report<'a> {
    pub health_score: u8,
    pub health_after_optimise: u8,
    pub memory: &'a MemInfo,
    pub commit_ratio: f64,
    pub level: Level,
    pub process_count: usize,
    pub top_apps: Vec<AppGroup>,
    pub recommendations: &'a [Recommendation],
    pub junk: &'a [Found],
    pub health_checks: &'a [Insight],
    pub facts: &'a Facts,
}

pub fn build(r: &ScanResult) -> Report<'_> {
    let (ratio, level) = pressure(&r.snap.mem);
    let mut top = group_apps(&r.snap.procs, r.snap.self_pid);
    top.truncate(25);
    Report {
        health_score: r.score,
        health_after_optimise: r.score_after,
        memory: &r.snap.mem,
        commit_ratio: ratio,
        level,
        process_count: r.snap.procs.len(),
        top_apps: top,
        recommendations: &r.recs,
        junk: &r.junk,
        health_checks: &r.insights,
        facts: &r.facts,
    }
}

pub fn headline(m: &MemInfo) -> String {
    let (ratio, level) = pressure(m);
    format!(
        "Memory pressure: {} ({} committed on {} of RAM, {:.0}%)",
        level.label(),
        fmt_bytes(m.commit_used),
        fmt_bytes(m.total_phys),
        ratio * 100.0
    )
}

pub fn text(r: &Report) -> String {
    let mut s = String::new();
    s.push_str("OneClick Optimizer report\n=========================\n");
    s.push_str(&format!(
        "Health score: {}/100 (about {} after Optimise)\n",
        r.health_score, r.health_after_optimise
    ));
    s.push_str(&headline(r.memory));
    s.push('\n');
    s.push_str(&format!(
        "Free right now: {}   Commit limit (RAM + page file): {}   Processes: {}\n\n",
        fmt_bytes(r.memory.avail_phys),
        fmt_bytes(r.memory.commit_limit),
        r.process_count
    ));
    s.push_str("Top apps by memory\n");
    for g in r.top_apps.iter().take(12) {
        s.push_str(&format!("  {:<34} {:>3} proc  {:>8}  {}\n", truncate(&g.name, 34), g.count, fmt_bytes(g.private_bytes), g.label));
    }

    s.push_str(&format!("\nFixes ({}):\n", r.recommendations.len()));
    for x in r.recommendations {
        s.push_str(&format!(
            "  [{}] {:<62} {:>8}  {}\n      {}\n",
            if x.preselect { "x" } else { " " },
            truncate(&x.title, 62),
            if x.saves_bytes > 0 { fmt_bytes(x.saves_bytes) } else { "at boot".into() },
            x.category.label(),
            x.why
        ));
    }
    if r.recommendations.is_empty() {
        s.push_str("  Nothing to fix right now.\n");
    }

    s.push_str("\nJunk files:\n");
    for j in r.junk {
        let size = if !j.measurable { "needs admin to measure".to_string() } else { format!("{} in {} file(s)", fmt_bytes(j.bytes), j.files) };
        s.push_str(&format!("  [{}] {:<40} {}\n", if j.default_on && j.bytes > 0 { "x" } else { " " }, j.name, size));
    }

    s.push_str("\nHealth checks:\n");
    for i in r.health_checks {
        s.push_str(&format!("  {:?}: {}\n      {}\n", i.severity, i.title, i.detail));
    }
    if r.health_checks.is_empty() {
        s.push_str("  All passed.\n");
    }
    s.push_str("\nOpen OneClickOptimizer.exe and press Optimise now to apply the ticked items.\n");
    s
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(n - 1).collect();
        t.push('\u{2026}');
        t
    }
}
