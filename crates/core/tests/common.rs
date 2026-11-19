#![allow(dead_code)]
use std::path::PathBuf;

pub use benchlens_core::testing::fixtures;

pub fn fixture(rel: &str) -> PathBuf {
    fixtures().join(rel)
}
