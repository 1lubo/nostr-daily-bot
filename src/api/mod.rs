//! API module for web endpoints.

mod handlers;
mod routes;
pub mod tips;

pub use routes::create_router;
