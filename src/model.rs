//! Plain data captured from the system. No Windows types here, so the
//! analysis can be tested with hand-made snapshots.

use serde::{Deserialize, Serialize};

pub const SERVICE_AUTO_START: u32 = 2;
pub const SERVICE_DEMAND_START: u32 = 3;
pub const SERVICE_DISABLED: u32 = 4;

pub fn start_type_name(t: u32) -> &'static str {
    match t {
        0 => "Boot",
        1 => "System",
        SERVICE_AUTO_START => "Automatic",
        SERVICE_DEMAND_START => "Manual",
        SERVICE_DISABLED => "Disabled",
        _ => "Unknown",
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MemInfo {
    pub total_phys: u64,
    pub avail_phys: u64,
    /// Memory programs have reserved (RAM + page file).
    pub commit_used: u64,
    pub commit_limit: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProcInfo {
    pub pid: u32,
    pub ppid: u32,
    /// Executable name as Windows reports it, e.g. `node.exe`.
    pub name: String,
    pub path: String,
    pub cmdline: String,
    pub private_bytes: u64,
    pub working_set: u64,
    /// Creation time as FILETIME ticks (0 if unknown).
    pub created: u64,
    /// CPU use during the sample window, in percent of the whole machine.
    pub cpu_pct: f32,
    pub ports: Vec<u16>,
    /// Open handles (files, registry keys, events...). Very high = possible leak.
    #[serde(default)]
    pub handles: u32,
    /// Change in private memory during the sample window (bytes).
    #[serde(default)]
    pub growth_bytes: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ServiceInfo {
    pub name: String,
    pub display: String,
    pub running: bool,
    pub start_type: u32,
    pub path: String,
    pub pid: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunLocation {
    HkcuRun,
    HklmRun,
    HklmRun32,
}

impl RunLocation {
    pub const ALL: [RunLocation; 3] = [Self::HkcuRun, Self::HklmRun, Self::HklmRun32];

    pub fn label(self) -> &'static str {
        match self {
            Self::HkcuRun => "This user",
            Self::HklmRun => "All users",
            Self::HklmRun32 => "All users (32-bit)",
        }
    }

    pub fn is_user(self) -> bool {
        matches!(self, Self::HkcuRun)
    }

    pub fn run_key(self) -> &'static str {
        match self {
            Self::HkcuRun | Self::HklmRun => r"Software\Microsoft\Windows\CurrentVersion\Run",
            Self::HklmRun32 => r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run",
        }
    }

    /// Where Task Manager stores the enabled/disabled flag.
    pub fn approved_key(self) -> &'static str {
        match self {
            Self::HkcuRun | Self::HklmRun => {
                r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
            }
            Self::HklmRun32 => {
                r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run32"
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StartupEntry {
    pub location: RunLocation,
    pub name: String,
    pub command: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub mem: MemInfo,
    pub procs: Vec<ProcInfo>,
    pub services: Vec<ServiceInfo>,
    pub startup: Vec<StartupEntry>,
    /// FILETIME ticks when the snapshot was taken.
    pub now: u64,
    pub self_pid: u32,
}
