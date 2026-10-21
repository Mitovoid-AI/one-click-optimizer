//! OneClick Optimizer: find what slows a Windows PC (memory hogs, idle dev
//! tools, junk files, startup bloat, health problems), explain it in plain
//! words, and fix it in one click with an undo journal.
//!
//! Pure logic (`kb`, `analyze`, `health`, `junk` rules, `actions` data types,
//! `util`) is unit tested; everything that touches Windows lives in `sys`,
//! `ui`, `agent`, `paint`, `icon` and `elevate`.

pub mod actions;
pub mod agent;
pub mod analyze;
pub mod elevate;
pub mod health;
pub mod icon;
pub mod install;
pub mod junk;
pub mod kb;
pub mod model;
pub mod optimize;
pub mod paint;
pub mod report;
pub mod setup;
pub mod sys;
pub mod ui;
pub mod util;

/// Product name shown everywhere.
pub const APP_NAME: &str = "OneClick Optimizer";
