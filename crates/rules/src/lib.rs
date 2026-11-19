//! benchlens-rules: everything benchlens decides, with no I/O, so the same code runs in the server, the
//! command line and the browser (WebAssembly).
//!
//! * [`catalog`]: the vmbench metric table per vmbench version
//! * [`verdict`]: median, CV and the better / worse / not clear rule (vmbench report.py `judge()`)
//! * [`compare`]: pooling values per side and judging every metric against the baseline

pub mod catalog;
pub mod compare;
pub mod verdict;
