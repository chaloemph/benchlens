//! benchlens-core: read vmbench result folders, keep them in SQLite and compare them with the same rule as
//! `vmbench report compare`.
//!
//! * [`contract`]: what a vmbench result folder holds (the only files benchlens relies on)
//! * [`catalog`]: the vmbench metric table per vmbench version
//! * [`verdict`]: median, CV and the better / worse / not clear rule (vmbench report.py `judge()`)
//! * [`store`]: the SQLite database and its views
//! * [`import`]: bringing folders and tar archives in safely
//! * [`compare`]: baseline against other runs or groups

pub mod catalog;
pub mod compare;
pub mod contract;
pub mod error;
pub mod import;
pub mod store;
pub mod verdict;

pub use error::{Error, Result};
