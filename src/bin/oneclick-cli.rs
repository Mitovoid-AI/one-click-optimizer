use optimizer::{actions::Journal, optimize, report, sys, util};

const HELP: &str = "OneClick Optimizer (command line)

USAGE:
  oneclick-cli [scan] [--json]   Deep scan: health score, fixes, junk files, health checks (read-only)
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
