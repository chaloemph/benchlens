# Wave 2 review: I (watch stops on signals, inbox registry and tags, inbox lock)

Branch `m2/i-inbox` at 008f7fa, reviewed against docs/plan.md (F3, F6, F11). Verdict: **ok, fixed in
review** (serve did not stop on SIGTERM; rust-version left inconsistent).

## What was checked

- F3: `watch` installs SIGINT and SIGTERM handlers on a small tokio runtime in its own thread; the first
  signal lets the import in progress finish (the scan loop checks `stop` between entries), the second exits
  130. Process tests send both signals and check exit 0, "stopped watching", and the lock record emptied.
- F6: `--registry` / `--tag` for `watch`, `--inbox-registry` / `--inbox-tag` for `serve`. The registry is read
  again when mtime or size changes; a broken file is reported once and the previous registry stays. Checked by
  hand on the POC: an r2 folder dropped into the inbox of `serve --inbox --inbox-registry` was imported with
  its registry tags (group, round, set, size, host) and moved to `done/`.
- F11 lock: `File::try_lock` (flock / LockFileEx) on `<inbox>/.benchlens.lock`, a dotfile so scans skip it;
  the record (pid, host, start) only names the holder and spots stale records. The OS lock goes away with the
  process; the file is emptied, never removed (no two-files race). No path from the lock file is followed or
  written outside the inbox. On a file system without advisory locks the record alone guards (documented).
- Untrusted input: the lock record is parsed leniently and only displayed; registry parsing is core's.

## Fixed in review

- `review(I)`: `benchlens serve` stopped only on Ctrl-C, so `docker stop` waited 10 s and killed it (leaving
  the lock record). `shutdown_signal()` ends the graceful shutdown on SIGINT or SIGTERM, then the inbox thread is
  stopped and joined. Process tests for both signals (exit within 5 s, lock released); `docker stop` now takes
  0.3 s with exit code 0.
- `review(I)`: core declared rust-version 1.89 while the workspace (cli, server) said 1.80. The workspace is now
  1.89; benchlens-rules keeps 1.80 and benchlens-wasm 1.81 (neither depends on core). Raising it enabled
  clippy's `unnecessary_map_or`; three `map_or(true, ..)` in read.rs became `is_none_or`.
- Merge with C2: serve's inbox registry and tags go through `config::Layer` (`BENCHLENS_INBOX_REGISTRY`,
  `inbox_registry` and `inbox_tags = { k = "v" }` in benchlens.toml) like the other serve settings.
- `refactor(I)`: `benchlens_server::prepare_inbox` (registry check, mkdir, lock, stale-lock warning) is shared
  by `watch` and `serve --inbox`.

## Notes and follow-ups

- Stale-record check uses `/proc/<pid>` (or `kill -0`) on the same host: after a crash, if the pid is reused by
  an unrelated process the next watcher refuses and says to remove the file. Rare; the message is clear.
- In a container the watcher is pid 1 with the container id as host; a restart of the same container takes
  over its own stale record (same pid), a new container with another hostname relies on the OS lock.
