mod home;
pub use crate::home::{default_notes_dir, home_dir};

mod document;
pub use crate::document::*;

mod plugin;
pub use crate::plugin::*;

pub mod config;
pub use crate::config::{Config, GitConfig};

pub mod git;
pub mod search;
