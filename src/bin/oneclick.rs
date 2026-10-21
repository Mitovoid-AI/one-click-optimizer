#![windows_subsystem = "windows"]

use optimizer::{agent, elevate, install, setup, ui};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let code = match args.get(1).map(String::as_str) {
        Some("agent") => {
            agent::run();
            0
        }
        Some("apply-plan") => elevate::run_plan_arg(args.get(2).map_or("", String::as_str)),
        Some("--install") | Some("--setup") => {
            setup::run();
            0
        }
        Some("--uninstall") => setup::uninstall_flow(args.iter().any(|a| a == "--quiet")),
        // The download is named OneClickOptimizer-Setup.exe: show the installer.
        _ if setup::should_show_setup() => {
            setup::run();
            0
        }
        _ => {
            let layout = install::Layout::standard();
            if install::running_installed_copy(&layout) {
                install::cleanup_old_copies(&layout);
            }
            ui::run();
            0
        }
    };
    std::process::exit(code);
}
