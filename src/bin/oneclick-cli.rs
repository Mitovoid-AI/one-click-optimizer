use optimizer::model::ProcInfo;
use optimizer::{actions::Journal, devclean, focus, optimize, report, sys, update, util};
use serde_json::json;
use std::time::{Duration, Instant};

const HELP: &str = "OneClick Optimizer (command line)

USAGE:
  oneclick-cli [scan] [--json]   Deep scan: health score, fixes, junk files, health checks (read-only)
  oneclick-cli dev [--json]      Developer junk: node_modules, build folders, package caches (read-only)
  oneclick-cli bench [--json]    Measures this PC (read-only scans) and how much Focus mode's
                                 efficiency mode helps a foreground task (uses ~30 s of CPU)
  oneclick-cli history           Changes OneClick Optimizer has made
  oneclick-cli help

Open OneClickOptimizer.exe for the window (one-click Optimise), or
`OneClickOptimizer.exe agent` for the tray watch.";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json = args.iter().any(|a| a == "--json");
    let cmd = args.iter().find(|a| !a.starts_with("--")).map(String::as_str).unwrap_or("scan");
    match cmd {
        "scan" => {
            let scan = optimize::deep_scan(&|_| {});
            let r = report::build(&scan);
            if json {
                println!("{}", serde_json::to_string_pretty(&r).expect("serializable report"));
            } else {
                print!("{}", report::text(&r));
            }
        }
        "dev" => {
            let items = devclean::scan();
            if json {
                let v: Vec<_> = items
                    .iter()
                    .map(|i| json!({ "path": i.path, "kind": format!("{:?}", i.kind), "project": i.project, "bytes": i.bytes, "files": i.files, "idle_days": i.idle_days }))
                    .collect();
                println!("{}", serde_json::to_string_pretty(&v).expect("serializable"));
            } else {
                let total: u64 = items.iter().map(|i| i.bytes).sum();
                println!("Developer junk: {} in {} folders\n", util::fmt_bytes(total), items.len());
                for i in &items {
                    let days = i.idle_days.map_or(String::new(), |d| format!("{d} days"));
                    println!("{:>9}  {:>9}  {}", util::fmt_bytes(i.bytes), days, i.path.display());
                }
            }
        }
        "bench" => bench(json),
        // Helper for `bench`: one busy background program.
        "busy" => loop {
            std::hint::black_box((0..100_000u64).fold(0u64, |a, b| a.wrapping_mul(31).wrapping_add(b)));
        },
        "history" => {
            let j = Journal::load(&util::journal_path());
            if j.entries.is_empty() {
                println!("No changes yet.");
            }
            for e in &j.entries {
                println!(
                    "#{:<4} {}  {:<60} {}{}",
                    e.id,
                    sys::mem::fmt_local(e.time),
                    e.action.describe(),
                    if e.ok { "OK: " } else { "FAILED: " },
                    e.message
                );
            }
        }
        "help" | "-h" | "/?" => println!("{HELP}"),
        other => {
            eprintln!("Unknown command `{other}`.\n\n{HELP}");
            std::process::exit(2);
        }
    }
}

/// "Vite dev server (my-shop)" -> "Vite dev server": project names stay private.
fn without_project(label: &str) -> String {
    match label.split_once(" dev server (") {
        Some((tool, _)) => format!("{tool} dev server"),
        None => label.to_string(),
    }
}

/// The foreground task: a fixed amount of hashing on one thread.
fn foreground_task() -> Duration {
    let data = vec![7u8; 16 << 20];
    let t = Instant::now();
    for _ in 0..64 {
        std::hint::black_box(update::sha256_hex(&data));
    }
    t.elapsed()
}

fn median(mut v: Vec<Duration>) -> u128 {
    v.sort();
    v[v.len() / 2].as_millis()
}

/// Starts `n` busy background programs (copies of this exe running `busy`).
fn start_busy(n: usize) -> Vec<std::process::Child> {
    let exe = std::env::current_exe().expect("own path");
    (0..n).filter_map(|_| std::process::Command::new(&exe).arg("busy").spawn().ok()).collect()
}

/// How much Focus mode's efficiency mode helps a foreground task when the PC
/// is busy: the same task alone, next to busy background programs, and next to
/// the same programs in efficiency mode. Only processes started here are touched.
fn focus_bench() -> serde_json::Value {
    const RUNS: usize = 5;
    let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
    let busy_n = cores * 2;
    foreground_task(); // warm up
    let alone = median((0..RUNS).map(|_| foreground_task()).collect());
    let mut children = start_busy(busy_n);
    std::thread::sleep(Duration::from_millis(500));
    let busy = median((0..RUNS).map(|_| foreground_task()).collect());
    let mut slowed = 0;
    for c in &children {
        let created = sys::procs::creation_time(c.id()).unwrap_or(0);
        let p = ProcInfo { pid: c.id(), created, name: "oneclick-cli.exe".into(), ..Default::default() };
        if focus::slow_down(&p).is_some() {
            slowed += 1;
        }
    }
    std::thread::sleep(Duration::from_millis(500));
    let efficient = median((0..RUNS).map(|_| foreground_task()).collect());
    for c in &mut children {
        let _ = c.kill();
        let _ = c.wait();
    }
    json!({ "cores": cores, "busy_programs": busy_n, "slowed": slowed, "alone_ms": alone, "busy_ms": busy, "efficiency_ms": efficient })
}

fn bench(as_json: bool) {
    let mem = sys::mem::read_mem();
    let t = Instant::now();
    let scan = optimize::deep_scan(&|_| {});
    let scan_ms = t.elapsed().as_millis();
    let idle: Vec<_> = scan
        .recs
        .iter()
        .filter(|r| r.preselect && r.saves_bytes > 0)
        .map(|r| match &r.action {
            optimizer::actions::Action::KillProcesses { label, .. } => json!({ "label": without_project(label), "bytes": r.saves_bytes }),
            _ => json!({ "label": r.title, "bytes": r.saves_bytes }),
        })
        .collect();
    let junk: Vec<_> = scan.junk.iter().filter(|j| j.bytes > 0).map(|j| json!({ "name": j.name, "bytes": j.bytes, "ticked": j.default_on })).collect();

    let t = Instant::now();
    let dev = devclean::scan();
    let dev_ms = t.elapsed().as_millis();
    let mut kinds: Vec<(String, u64, usize)> = Vec::new();
    for i in &dev {
        let name = if i.kind == devclean::Kind::PackageCache { "Package caches".to_string() } else { i.label.clone() };
        match kinds.iter_mut().find(|k| k.0 == name) {
            Some(k) => {
                k.1 += i.bytes;
                k.2 += 1;
            }
            None => kinds.push((name, i.bytes, 1)),
        }
    }
    kinds.sort_by(|a, b| b.1.cmp(&a.1));
    let stale: u64 = dev.iter().filter(|i| i.default_on()).map(|i| i.bytes).sum();

    let focus = focus_bench();
    let out = json!({
        "machine": { "cores": std::thread::available_parallelism().map_or(0, |n| n.get()), "ram_bytes": mem.total_phys },
        "memory": { "in_use_bytes": mem.commit_used, "ram_bytes": mem.total_phys, "free_bytes": mem.avail_phys },
        "scan": { "ms": scan_ms, "score": scan.score, "score_after": scan.score_after, "idle_programs": idle, "junk": junk },
        "dev": { "ms": dev_ms, "folders": dev.len(), "total_bytes": dev.iter().map(|i| i.bytes).sum::<u64>(), "stale_bytes": stale,
                 "kinds": kinds.iter().map(|(k, b, n)| json!({ "kind": k, "bytes": b, "count": n })).collect::<Vec<_>>() },
        "focus": focus,
    });
    if as_json {
        println!("{}", serde_json::to_string_pretty(&out).expect("serializable"));
    } else {
        println!("{out:#}");
    }
}
