#![allow(dead_code)]
use std::path::PathBuf;

pub fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

pub fn fixture(rel: &str) -> PathBuf {
    fixtures().join(rel)
}
