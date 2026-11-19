//! The repository's test fixtures, shared by the tests and examples of every crate. Not used by the binaries
//! (the paths point into the source checkout).

use crate::import::{import_path, ImportOptions, ImportReport, Registry};
use crate::store::Store;
use std::path::PathBuf;

/// vmbench versions with a folder of result folders under `tests/fixtures/`.
pub const FIXTURE_VERSIONS: [&str; 2] = ["v0.2.1", "v0.2.3"];

/// The root of the source checkout (where `tests/` and `examples/` are).
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `tests/fixtures/`.
pub fn fixtures() -> PathBuf {
    repo_root().join("tests/fixtures")
}

/// `examples/poc-2026-11/registry.tsv`, parsed.
pub fn poc_registry() -> Registry {
    let path = repo_root().join("examples/poc-2026-11/registry.tsv");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    Registry::parse(&text).expect("examples/poc-2026-11/registry.tsv parses")
}

/// Import every fixture version into `store` with `opts`; panics when an archive or folder is refused.
pub fn import_fixtures(store: &Store, opts: &ImportOptions) -> Vec<ImportReport> {
    let mut reports = Vec::new();
    for ver in FIXTURE_VERSIONS {
        reports.extend(
            import_path(store, &fixtures().join(ver), opts)
                .unwrap_or_else(|e| panic!("importing fixtures {ver}: {e}")),
        );
    }
    reports
}

/// An in-memory store holding every fixture, tagged with the POC registry when `with_registry`.
pub fn fixture_store(with_registry: bool) -> Store {
    let store = Store::open_in_memory().expect("in-memory store");
    let opts = ImportOptions {
        registry: with_registry.then(poc_registry),
        ..ImportOptions::default()
    };
    import_fixtures(&store, &opts);
    store
}
