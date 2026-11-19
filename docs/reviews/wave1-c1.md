# Wave 1 review: C1 (inbox, `benchlens watch`, `serve --inbox`)

Branch `m2/c1-inbox` at fbe3b39. Verdict: **ok, fixed in review** (message only).

## What was checked

- Only top-level entries are considered: directories and regular `.tar` / `.tar.gz` / `.tgz` files. Links,
  sockets, devices, dotfiles, `done/`, `failed/`, `.part` / `.tmp` / `.partial` and non-UTF-8 names are left
  alone; `file_type()` and `symlink_metadata` never follow links. The import itself is core's, which refuses
  unsafe archives and links.
- Stability: an entry is imported only when size, file count and newest mtime are unchanged between two scans.
- Moves never overwrite: `create_new` for the reason file and an existence check before `rename`, with a
  numbered name when taken. Nothing is deleted. Entries that cannot be moved are marked stuck until they change.
- `serve --inbox` runs the watcher in its own thread with its own SQLite connection (WAL, rusqlite's 5 s busy
  timeout) and stops it on shutdown.
- Tests (`crates/core/tests/inbox.rs`, 7): folder after two scans, growing file waits, broken folder and junk
  tar go to `failed/` with a reason, archive with two runs, same run twice is `already_present`, names never
  overwritten, links/dotfiles/partials ignored. They test their claims.
- End to end on the release binary: `serve --inbox DIR --inbox-interval 1`, a run deleted through the API,
  its tar dropped as `*.tar.gz` (written as `.part`, then renamed) came back with the same run id; a broken
  folder went to `failed/` with `.reason.txt`.

## Fixed in review

- `review(C1)`: the failure reason repeated the folder name ("half: half: half: missing results.tsv ..." on
  the console, "half: half: ..." in the reason file), because core's error already starts with it.

## Deliberate differences from the plan (accepted, plan updated)

- Polling only, no `notify` crate: the stability rule needs a second look after a delay anyway, and polling
  works the same on network shares and in containers.
- `ServeConfig` has two new required fields, `inbox` and `inbox_interval`.

## Follow-ups (in the plan)

- `watch` has no Ctrl-C handling. Safe as it is (an import is one transaction, the move comes after; an
  interrupted entry is imported again as `already_present` and moved), but it should stop between scans.
- One watcher per inbox is assumed (two processes on one inbox could race on the rename). Document or lock.
- Inbox imports carry no registry tags: a deleted run dropped into the inbox again comes back with its
  automatic group, not its registry group (seen in the E2E run). A `--registry` for `watch` / `serve` (or a
  registry file inside the inbox) would fix it.
- An entry whose fingerprint cannot be read logs an error on every scan.
