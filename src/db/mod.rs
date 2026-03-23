//! Database module for SQLite persistence.

pub mod challenges;
pub mod history;
mod pool;
pub mod quotes;
pub mod signed_events;
pub mod users;

pub use pool::{init_db, DbPool};
