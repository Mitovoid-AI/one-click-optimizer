//! End-to-end tests of apply + journal + undo against harmless targets:
//! a throwaway `ping` process and a made-up startup entry name.

use optimizer::actions::{execute, Action, Journal, Op, Previous, ProcTarget};
use optimizer::model::RunLocation;
use optimizer::sys;
use std::path::PathBuf;

fn temp_journal(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oco-it-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("journal.json");
    let _ = std::fs::remove_file(&p);
    p
}

#[test]
fn kill_is_journaled_and_not_undoable() {
    let journal = temp_journal("kill");
    let mut child = std::process::Command::new("cmd").args(["/c", "ping -n 60 127.0.0.1 >nul"]).spawn().unwrap();
    let pid = child.id();
    let created = sys::procs::creation_time(pid).unwrap();
    let action = Action::KillProcesses {
        label: "test ping".into(),
        targets: vec![ProcTarget { pid, name: "cmd.exe".into(), created }],
    };

    let left = execute(&journal, vec![Op::Apply { action }], false);
    assert!(left.is_empty(), "nothing should need admin");
    let status = child.wait().unwrap();
    assert!(!status.success(), "process was ended");

    let j = Journal::load(&journal);
    assert_eq!(j.entries.len(), 1);
    assert!(j.entries[0].ok, "{}", j.entries[0].message);
    assert_eq!(j.entries[0].message, "ended 1");

    // Undo of a kill records a failure and leaves the entry as-is.
    execute(&journal, vec![Op::Undo { id: 1 }], false);
    let j = Journal::load(&journal);
    assert_eq!(j.entries.len(), 2);
    assert!(!j.entries[1].ok);
    assert!(!j.entries[0].undone);
}

#[test]
fn protected_and_self_are_never_killed() {
    let journal = temp_journal("protect");
    let me = std::process::id();
    let action = Action::KillProcesses {
        label: "bad".into(),
        targets: vec![
            ProcTarget { pid: me, name: "anything.exe".into(), created: 0 },
            ProcTarget { pid: 4, name: "System".into(), created: 0 },
        ],
    };
    execute(&journal, vec![Op::Apply { action }], false);
    let j = Journal::load(&journal);
    assert!(j.entries[0].message.contains("2 protected, skipped"), "{}", j.entries[0].message);
}

#[test]
fn startup_toggle_and_undo_roundtrip() {
    let journal = temp_journal("startup");
    let name = "OneClick-it-fake-entry";
    let loc = RunLocation::HkcuRun;
    let _ = sys::startup::delete_approved(loc, name);

    let action = Action::SetStartupEnabled { location: loc, value_name: name.into(), enabled: false };
    assert!(execute(&journal, vec![Op::Apply { action }], false).is_empty());
    let bytes = sys::startup::read_approved(loc, name).unwrap().expect("flag written");
    assert!(!sys::startup::approved_enabled(Some(&bytes)));

    let j = Journal::load(&journal);
    assert_eq!(j.entries[0].previous, Previous::StartupApproved { bytes: None });

    execute(&journal, vec![Op::Undo { id: 1 }], false);
    assert_eq!(sys::startup::read_approved(loc, name).unwrap(), None, "undo restored 'no flag'");
    let j = Journal::load(&journal);
    assert!(j.entries[0].undone);
    assert_eq!(j.entries[1].undo_of, Some(1));
    assert!(j.entries[1].ok);

    // Undoing twice is a no-op.
    execute(&journal, vec![Op::Undo { id: 1 }], false);
    assert_eq!(Journal::load(&journal).entries.len(), 2);
}

#[test]
fn admin_changes_are_deferred_when_not_elevated() {
    if sys::is_elevated() {
        return; // running as admin: nothing would be deferred
    }
    let journal = temp_journal("defer");
    let ops = vec![
        Op::Apply {
            action: Action::SetServiceStart { name: "Spooler".into(), display: "Print Spooler".into(), start: 3, stop: false },
        },
        Op::Apply {
            action: Action::SetStartupEnabled { location: RunLocation::HklmRun, value_name: "X".into(), enabled: false },
        },
    ];
    let left = execute(&journal, ops.clone(), false);
    assert_eq!(left, ops, "both need admin and must be handed to the elevated helper untouched");
    assert!(Journal::load(&journal).entries.is_empty());
}
