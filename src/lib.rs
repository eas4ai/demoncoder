pub mod adapters;
mod chat;
pub mod config;
mod context;
mod developer_access;
pub mod events;
mod export_policy;
mod highlight;
mod inspection;
pub mod language_services;
mod learning;
pub mod native;
pub mod oracle;
mod selection;
pub mod session;
mod socket_filter;
pub mod startup;
pub mod status;
pub mod subagents;
pub mod supervisor;
pub mod terminal;
pub mod tools;
mod transcript;
pub mod workflow;
mod worktree_access;

pub mod settings;

pub mod plugins;
#[cfg(test)]
extern crate self as demoncoder;
#[cfg(test)]
#[path = "../tests/support/config_change_runners.rs"]
pub(crate) mod config_change_test_support;
