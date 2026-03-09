//! Database module for SQLite persistence.

mod pool;
pub mod users;
pub mod quotes;
pub mod history;

pub use pool::{init_db, DbPool};

