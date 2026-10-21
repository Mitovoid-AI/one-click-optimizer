//! Changes OneClick Optimizer can make, the undo journal, and the executor that
//! runs them (deferring anything that needs admin rights to the elevated helper).

use crate::model::{start_type_name, RunLocation};
use crate::util::fmt_bytes;
use crate::{junk, kb, sys};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const ERROR_ACCESS_DENIED: u32 = 5;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcTarget {
    pub pid: u32,
    pub name: String,
    /// Creation FILETIME; guards against killing a recycled PID.
    pub created: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    KillProcesses {
        label: String,
        targets: Vec<ProcTarget>,
    },
    SetServiceStart {
        name: String,
        display: String,
        start: u32,
        stop: bool,
    },
    SetStartupEnabled {
        location: RunLocation,
        value_name: String,
        enabled: bool,
    },
    /// Delete one junk-file category (see `junk::CATEGORIES`).
    CleanJunk {
        id: String,
        label: String,
    },
}

impl Action {
    pub fn describe(&self) -> String {
        match self {
            Action::KillProcesses { label, targets } => {
                let n = targets.len();
                format!("End {label} ({n} process{})", if n == 1 { "" } else { "es" })
            }
            Action::SetServiceStart { display, start, stop, .. } => format!(
                "Set service \"{display}\" to {}{}",
                start_type_name(*start),
                if *stop { " and stop it" } else { "" }
            ),
            Action::SetStartupEnabled { value_name, enabled, .. } => format!(
                "{} \"{value_name}\" at startup",
                if *enabled { "Enable" } else { "Disable" }
            ),
            Action::CleanJunk { label, .. } => format!("Clean {label}"),
        }
    }

    pub fn needs_admin(&self) -> bool {
        match self {
            Action::KillProcesses { .. } => false,
            Action::SetServiceStart { .. } => true,
            Action::SetStartupEnabled { location, .. } => !location.is_user(),
            Action::CleanJunk { id, .. } => junk::category(id).is_some_and(|c| c.admin),
        }
    }

    pub fn undoable(&self) -> bool {
        !matches!(self, Action::KillProcesses { .. } | Action::CleanJunk { .. })
    }
}

/// State before a change, so it can be undone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Previous {
    Nothing,
    ServiceStart { start: u32 },
    StartupApproved { bytes: Option<Vec<u8>> },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JournalEntry {
    pub id: u64,
    /// FILETIME ticks.
    pub time: u64,
    pub action: Action,
    pub previous: Previous,
    pub ok: bool,
    pub message: String,
    #[serde(default)]
    pub undone: bool,
    /// Set on entries that record an undo of another entry.
    #[serde(default)]
    pub undo_of: Option<u64>,
    /// Disk space freed (junk cleaning).
    #[serde(default)]
    pub freed_bytes: u64,
}

#[derive(Debug, Default)]
pub struct Journal {
    pub path: PathBuf,
    pub entries: Vec<JournalEntry>,
}

impl Journal {
    pub fn load(path: &Path) -> Journal {
        let entries = match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|_| {
                // Keep a corrupt journal for inspection instead of silently dropping it.
                let _ = std::fs::rename(path, path.with_extension("corrupt.json"));
                Vec::new()
            }),
            Err(_) => Vec::new(),
        };
        Journal { path: path.to_path_buf(), entries }
    }

    pub fn save(&self) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&self.entries)?)?;
        std::fs::rename(&tmp, &self.path)
    }

    pub fn next_id(&self) -> u64 {
        self.entries.iter().map(|e| e.id).max().unwrap_or(0) + 1
    }

    pub fn get(&self, id: u64) -> Option<&JournalEntry> {
        self.entries.iter().find(|e| e.id == id)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    Apply { action: Action },
    Undo { id: u64 },
}

/// What the elevated helper receives.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Plan {
    pub journal: PathBuf,
    pub ops: Vec<Op>,
}

pub struct Outcome {
    pub ok: bool,
    pub message: String,
    pub previous: Previous,
    pub access_denied: bool,
    pub freed_bytes: u64,
}

impl Outcome {
    fn ok(message: String, previous: Previous) -> Self {
        Outcome { ok: true, message, previous, access_denied: false, freed_bytes: 0 }
    }
    fn fail(code: u32, what: &str) -> Self {
        Outcome {
            ok: false,
            message: format!("{what} failed: {}", win_error_text(code)),
            previous: Previous::Nothing,
            access_denied: code == ERROR_ACCESS_DENIED,
            freed_bytes: 0,
        }
    }
}

pub fn win_error_text(code: u32) -> String {
    match code {
        2 => "not found".into(),
        5 => "access denied (needs admin)".into(),
        1060 => "service does not exist".into(),
        1061 | 1052 => "service cannot accept that request right now".into(),
        _ => format!("Windows error {code}"),
    }
}

fn apply_action(a: &Action, self_pid: u32) -> Outcome {
    match a {
        Action::KillProcesses { targets, .. } => kill(targets, self_pid),
        Action::SetServiceStart { name, start, stop, .. } => {
            let prev = match sys::services::query_start_type(name) {
                Ok(p) => p,
                Err(e) => return Outcome::fail(e, "Reading service"),
            };
            if let Err(e) = sys::services::set_start_type(name, *start) {
                return Outcome::fail(e, "Changing start type");
            }
            let mut msg = format!("{} → {}", start_type_name(prev), start_type_name(*start));
            if *stop {
                match sys::services::stop(name) {
                    Ok(()) => msg.push_str(", stopped"),
                    Err(e) => msg.push_str(&format!(
                        ", could not stop it now ({}); it won't start at next boot",
                        win_error_text(e)
                    )),
                }
            }
            Outcome::ok(msg, Previous::ServiceStart { start: prev })
        }
        Action::SetStartupEnabled { location, value_name, enabled } => {
            let prev = match sys::startup::read_approved(*location, value_name) {
                Ok(p) => p,
                Err(e) => return Outcome::fail(e, "Reading startup state"),
            };
            let bytes = sys::startup::approved_bytes(prev.as_deref(), *enabled, sys::mem::now_filetime());
            match sys::startup::write_approved(*location, value_name, &bytes) {
                Ok(()) => Outcome::ok(
                    if *enabled { "enabled".into() } else { "disabled".into() },
                    Previous::StartupApproved { bytes: prev },
                ),
                Err(e) => Outcome::fail(e, "Changing startup state"),
            }
        }
        Action::CleanJunk { id, .. } => match junk::clean(id) {
            Ok(r) => {
                let mut msg = format!("removed {} file(s), {}", r.files, fmt_bytes(r.bytes));
                if r.skipped > 0 {
                    msg.push_str(&format!("; {} in use were left alone", r.skipped));
                }
                Outcome { freed_bytes: r.bytes, ..Outcome::ok(msg, Previous::Nothing) }
            }
            Err(e) => Outcome { ok: false, message: e, previous: Previous::Nothing, access_denied: false, freed_bytes: 0 },
        },
    }
}

fn kill(targets: &[ProcTarget], self_pid: u32) -> Outcome {
    let (mut ended, mut gone, mut denied, mut skipped, mut failed) = (0, 0, 0, 0, 0);
    for t in targets {
        if t.pid == self_pid || kb::is_protected_process(&t.name) {
            skipped += 1;
            continue;
        }
        match sys::procs::terminate(t.pid, t.created) {
            Ok(()) => ended += 1,
            Err(sys::procs::GONE) => gone += 1,
            Err(ERROR_ACCESS_DENIED) => denied += 1,
            Err(_) => failed += 1,
        }
    }
    let mut parts = vec![format!("ended {ended}")];
    if gone > 0 {
        parts.push(format!("{gone} had already exited"));
    }
    if skipped > 0 {
        parts.push(format!("{skipped} protected, skipped"));
    }
    if denied > 0 {
        parts.push(format!("{denied} access denied"));
    }
    if failed > 0 {
        parts.push(format!("{failed} failed"));
    }
    Outcome {
        ok: denied == 0 && failed == 0,
        message: parts.join(", "),
        previous: Previous::Nothing,
        access_denied: denied > 0,
        freed_bytes: 0,
    }
}

fn undo_entry(e: &JournalEntry) -> Outcome {
    match (&e.action, &e.previous) {
        (Action::SetServiceStart { name, .. }, Previous::ServiceStart { start }) => {
            match sys::services::set_start_type(name, *start) {
                Ok(()) => Outcome::ok(format!("restored to {}", start_type_name(*start)), Previous::Nothing),
                Err(c) => Outcome::fail(c, "Restoring service"),
            }
        }
        (Action::SetStartupEnabled { location, value_name, .. }, Previous::StartupApproved { bytes }) => {
            let r = match bytes {
                Some(b) => sys::startup::write_approved(*location, value_name, b),
                None => sys::startup::delete_approved(*location, value_name),
            };
            match r {
                Ok(()) => Outcome::ok("startup setting restored".into(), Previous::Nothing),
                Err(c) => Outcome::fail(c, "Restoring startup setting"),
            }
        }
        _ => Outcome {
            ok: false,
            message: "this change cannot be undone (closed apps start again when you open them; deleted junk is gone)".into(),
            previous: Previous::Nothing,
            access_denied: false,
            freed_bytes: 0,
        },
    }
}

fn op_needs_admin(op: &Op, j: &Journal) -> bool {
    match op {
        Op::Apply { action } => action.needs_admin(),
        Op::Undo { id } => j.get(*id).is_some_and(|e| e.action.needs_admin()),
    }
}

/// Runs `ops`, recording each in the journal. When `elevated` is false, ops
/// that need admin (or hit "access denied") are returned for the elevated helper.
pub fn execute(journal_path: &Path, ops: Vec<Op>, elevated: bool) -> Vec<Op> {
    let mut j = Journal::load(journal_path);
    let can_admin = elevated || sys::is_elevated();
    let self_pid = std::process::id();
    let mut deferred = Vec::new();

    for op in ops {
        if !can_admin && op_needs_admin(&op, &j) {
            deferred.push(op);
            continue;
        }
        let (action, outcome, undo_of) = match &op {
            Op::Apply { action } => (action.clone(), apply_action(action, self_pid), None),
            Op::Undo { id } => match j.get(*id) {
                Some(e) if e.undone => continue,
                Some(e) => (e.action.clone(), undo_entry(e), Some(*id)),
                None => continue,
            },
        };
        if outcome.access_denied && !can_admin {
            deferred.push(op);
            continue;
        }
        if let (Some(id), true) = (undo_of, outcome.ok) {
            if let Some(orig) = j.entries.iter_mut().find(|e| e.id == id) {
                orig.undone = true;
            }
        }
        let entry = JournalEntry {
            id: j.next_id(),
            time: sys::mem::now_filetime(),
            action,
            previous: outcome.previous,
            ok: outcome.ok,
            message: if undo_of.is_some() {
                format!("Undo: {}", outcome.message)
            } else {
                outcome.message
            },
            undone: false,
            undo_of,
            freed_bytes: outcome.freed_bytes,
        };
        j.entries.push(entry);
        let _ = j.save();
    }
    deferred
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_actions() -> Vec<Action> {
        vec![
            Action::KillProcesses {
                label: "Gradle daemon".into(),
                targets: vec![ProcTarget { pid: 14796, name: "java.exe".into(), created: 1 }],
            },
            Action::SetServiceStart { name: "AGMService".into(), display: "Adobe".into(), start: 3, stop: true },
            Action::SetStartupEnabled { location: RunLocation::HklmRun, value_name: "X".into(), enabled: false },
        ]
    }

    #[test]
    fn admin_requirements() {
        let a = sample_actions();
        assert!(!a[0].needs_admin());
        assert!(a[1].needs_admin());
        assert!(a[2].needs_admin());
        let user = Action::SetStartupEnabled { location: RunLocation::HkcuRun, value_name: "X".into(), enabled: false };
        assert!(!user.needs_admin());
        assert!(!a[0].undoable() && a[1].undoable() && a[2].undoable());
    }

    #[test]
    fn plan_and_journal_serde_roundtrip() {
        let plan = Plan {
            journal: PathBuf::from(r"C:\x\journal.json"),
            ops: sample_actions().into_iter().map(|action| Op::Apply { action }).chain([Op::Undo { id: 7 }]).collect(),
        };
        let back: Plan = serde_json::from_slice(&serde_json::to_vec(&plan).unwrap()).unwrap();
        assert_eq!(back.ops, plan.ops);

        let e = JournalEntry {
            id: 1,
            time: 2,
            action: sample_actions()[2].clone(),
            previous: Previous::StartupApproved { bytes: Some(vec![2, 0, 0]) },
            ok: true,
            message: "disabled".into(),
            undone: false,
            undo_of: None,
            freed_bytes: 0,
        };
        let s = serde_json::to_string(&vec![e]).unwrap();
        let back: Vec<JournalEntry> = serde_json::from_str(&s).unwrap();
        assert_eq!(back[0].previous, Previous::StartupApproved { bytes: Some(vec![2, 0, 0]) });
    }

    #[test]
    fn journal_load_missing_and_corrupt() {
        let dir = std::env::temp_dir().join(format!("oco-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("journal.json");
        assert!(Journal::load(&p).entries.is_empty());
        std::fs::write(&p, b"not json").unwrap();
        assert!(Journal::load(&p).entries.is_empty());
        assert!(p.with_extension("corrupt.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
