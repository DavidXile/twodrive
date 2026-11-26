//! Google desktop OAuth and Drive adapter. Secrets and protocol details stay here.
pub mod auth;
mod drive;
pub use drive::{GoogleDrive, GoogleError};
