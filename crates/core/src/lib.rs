//! benchlens-core: read vmbench result folders, keep them in SQLite and compare them with the same rule as
//! `vmbench report compare`.
//!
//! * [`contract`]: what a vmbench result folder holds (the only files benchlens relies on)
//! * [`catalog`], [`verdict`]: re-exported from `benchlens-rules` (no I/O, also built for WebAssembly)
//! * [`store`]: the SQLite database and its views
//! * [`import`]: bringing folders and tar archives in safely
//! * [`compare`]: baseline against other runs or groups

pub mod compare;
pub mod contract;
pub mod error;
pub mod import;
pub mod store;

pub use benchlens_rules::{catalog, verdict};

pub use error::{Error, Result};
