# benchlens

Look at, compare and share [vmbench](https://github.com/chaloemph/vmbench) results.

benchlens imports vmbench result folders into a SQLite file and compares machines metric by metric with
**the same rule as `vmbench report compare`**, so its "better / worse / within noise" always agrees with
vmbench's own reports. Compare one run with another, or pool repeated runs of a machine as one group.

> Status: M1-M3 done: command line, web UI and JSON API, inbox folder, upload with token auth, static HTML /
> JSON / CSV export. Next: the v0.1.0 release (a dry run of the release workflow first). See
> [docs/plan.md](docs/plan.md) and [docs/api.md](docs/api.md).

## Install

benchlens is one binary with the web UI built in; the only state is the SQLite file.

### From a release

Download the archive for your platform and `SHA256SUMS` from
[Releases](https://github.com/chaloemph/benchlens/releases). Targets: `x86_64-unknown-linux-musl`,
`aarch64-unknown-linux-musl` (static, any distro), `aarch64-apple-darwin`, `x86_64-apple-darwin`,
`x86_64-pc-windows-msvc` (zip).

```bash
VERSION=0.1.0 TARGET=x86_64-unknown-linux-musl
curl -LO https://github.com/chaloemph/benchlens/releases/download/v$VERSION/benchlens-$VERSION-$TARGET.tar.gz
curl -LO https://github.com/chaloemph/benchlens/releases/download/v$VERSION/SHA256SUMS
sha256sum --check --ignore-missing SHA256SUMS     # macOS: shasum -a 256 --check --ignore-missing SHA256SUMS
tar -xzf benchlens-$VERSION-$TARGET.tar.gz
install -m 0755 benchlens-$VERSION-$TARGET/benchlens ~/.local/bin/
```

On Windows: `Get-FileHash benchlens-<version>-x86_64-pc-windows-msvc.zip` and compare with `SHA256SUMS`.

### From source

Rust 1.89 or later (and a C compiler, for the bundled SQLite):

```bash
cargo install --path crates/cli
```

### Docker

The image (`ghcr.io/chaloemph/benchlens`, linux/amd64 and linux/arm64) runs `benchlens serve` as a
non-root user on port 8080 with the database at `/data/benchlens.db`:

```bash
docker run -d --name benchlens -p 8080:8080 -v benchlens-data:/data \
  -e BENCHLENS_TOKEN="$(openssl rand -hex 32)" ghcr.io/chaloemph/benchlens
# import: make the results readable in the container, then run the same image with another command
docker run --rm -v benchlens-data:/data -v "$PWD/results:/results:ro" ghcr.io/chaloemph/benchlens import /results
```

The container listens on `0.0.0.0`, which is not loopback: **without `BENCHLENS_TOKEN` it is read-only**
(upload, tag editing and delete answer 403; reading, the web UI and `import` through `docker run` still work).
With a token, the web UI asks for it once per tab. See [Running on a server](#running-on-a-server).
`docker stop` stops it cleanly (SIGTERM).

Or with Compose, from a checkout ([docker-compose.yml](docker-compose.yml)):

```bash
BENCHLENS_TOKEN=$(openssl rand -hex 32) docker compose up -d     # http://localhost:8080
docker compose build                 # build the image locally instead of pulling it
```

## Use

```bash
# import result folders, a directory of them, or .tar / .tar.gz archives
benchlens import ~/bench-results/
benchlens import x86-std-20261118-233315.tar.gz --tag set=poc --tag round=1

# many runs at once with a registry: a TSV with a `run` column (folder name) and one column per tag
benchlens import --registry registry.tsv results/

benchlens runs                                    # what is in the database
benchlens compare x86-high-r2 arm64-high-r2       # run against run
benchlens compare group:x86-std group:x86-med group:x86-high   # groups: all runs of a machine pooled
benchlens compare x86-high x86-high-r2 --section pg --json
benchlens tag x86-high-r2 group=poc-x86-high exclude=web        # an empty value removes a tag
benchlens serve                                   # http://127.0.0.1:8080 (API under /api)

# share a comparison: one HTML file that opens from disk, offline (baseline and sides can still be changed)
benchlens export html --base group:x86-high --other group:arm64-high -o report.html --title "POC high"
benchlens export json group:x86-high group:arm64-high -o values.json   # sides, values, stats per metric
benchlens export csv group:x86-high group:arm64-high -o values.csv     # one row per raw value
```

The exported HTML inlines the web UI's compare, metric and hardware views, d3, the WebAssembly build of the
verdict rule and the values of the exported sides (about 750 KB, 230 KB gzipped, for two POC groups), so its
numbers equal `benchlens compare`; the Export button of the compare view downloads the same file. The CSV
columns are `side,run_id,label,id,name,section,unit,direction,round,value` (`side` is the reference as given,
`label` the run's label, `round` the repeat inside the run, `direction` empty for a vmbench version without a
metric table; text starting with `= + - @` gets a leading `'` so spreadsheets do not run it as a formula). The
JSON holds per side `side`, `stats` (`id`, `name`, `section`, `unit`, `direction`, `tool`, `n`, `median`,
`cv`, `min`, `max`), `unjudged` and `values` (`ValueRow`, docs/api.md).

The database is `benchlens.db` in the current folder unless `--db` or `BENCHLENS_DB` says otherwise.
Importing the same run again, by any route, does not add it twice.
Output piped into a reader that stops early (`benchlens compare ... | head`) ends the command quietly, killed
by SIGPIPE like other Unix tools (the shell reports status 141; with `set -o pipefail` that counts as a failure).

### Groups

A group is the sample a machine is judged on. By default a run's group is its label without round or rerun
suffixes (`x86-std-r2` and `x86-std-web` join `x86-std`) plus its CPU, vCPU and RAM, so rounds of one
machine pool together and two people's `x86-std` do not. A `group` tag overrides it.

### Tags

Any `key=value`. Give them with `--tag`, a registry, a `benchlens-tags.json` in the result folder, or
`benchlens tag` later (command line wins over the file, the file over the registry). Two keys mean something:

- `group`: the group, see above
- `exclude`: stages whose values are not used, e.g. `web,net` (stages that failed are always left out)

### Inbox

`benchlens watch` imports whatever is dropped into a folder; `benchlens serve --inbox DIR` does the same in
the background of the server.

```bash
benchlens watch --inbox inbox/ --registry registry.tsv --tag set=poc     # Ctrl-C to stop
benchlens serve --inbox inbox/ --registry registry.tsv --inbox-tag set=poc
```

- Drop result folders, directories of them, or `.tar` / `.tar.gz` / `.tgz` archives at the top of the inbox.
  An entry is imported once it stayed unchanged for one scan interval (`--interval` / `--inbox-interval`,
  5 s), then moved to `done/`, or to `failed/` with a `.reason.txt`. Upload under a name ending in `.part`,
  `.tmp` or `.partial` and rename when complete; dotfiles and links are ignored.
- `--registry FILE` (env `BENCHLENS_INBOX_REGISTRY` for `watch`; for `serve` also `BENCHLENS_REGISTRY` and
  `registry` in benchlens.toml, old name `--inbox-registry` / `inbox_registry` still accepted) and `--tag`
  (`--inbox-tag`) tag inbox imports as `benchlens import` does, so a run dropped in again keeps its registry
  group. The registry is read again whenever it changes, so rows can be added while the watcher runs; a file
  that no longer parses is reported once and the previous registry stays in use.
- One watcher per inbox: the watcher holds an advisory lock on `<inbox>/.benchlens.lock` (the file names the
  holder's pid, host and start time). A second `watch` or `serve --inbox` on the same folder refuses to start
  and names the holder. A lock left by a watcher that was killed is taken over with a warning; the OS
  releases the lock with the process. On a network share whose file system ignores advisory locks, run only
  one watcher per inbox.
- `watch` stops on Ctrl-C or SIGTERM after the import in progress and exits 0; a second Ctrl-C stops at once
  (an import is one transaction, so an interrupted entry is imported on the next run as already present).

## Running on a server

`benchlens serve` reads its settings from flags, then `BENCHLENS_*` variables, then an optional
`benchlens.toml` (`--config FILE` / `BENCHLENS_CONFIG`, else `./benchlens.toml` when present):

```toml
db = "/data/benchlens.db"
listen = "0.0.0.0:8080"
token = "..."            # or BENCHLENS_TOKEN; at least 16 characters, e.g. `openssl rand -hex 32`
read_public = true       # false: reading needs the token too (the page still loads and asks for it)
max_upload_mb = 256      # largest archive accepted by upload
# allowed_hosts = ["bench.example.com"]   # or BENCHLENS_ALLOWED_HOSTS=a,b: Host names served (see below)
# registry = "/data/registry.tsv"   # tags for inbox imports and uploads (old name: inbox_registry)
# inbox = "/data/inbox"  # optional, with inbox_interval = 5
# inbox_tags = { set = "poc" }
```

- **Token**: with a token, uploading, tag editing and delete need it (`Authorization: Bearer <token>`; the
  web UI asks once per tab). Without a token, writes are open only on a loopback address; on `0.0.0.0` or
  any other address they are refused and the server warns at start. The Docker image listens on
  `0.0.0.0:8080`, so pass `-e BENCHLENS_TOKEN=...` to upload or edit through it. There is no `--token` flag
  (command lines are visible to other users); the token is never printed.
- **read_public = false** keeps the data private: every `/api` call except `/api/health` needs the token.
  The server refuses to start like that without a token.
- **Host names** (DNS rebinding guard): without a token, the server answers only requests addressed to
  `localhost` or an IP address (`127.0.0.1`, `[::1]`, `192.168.1.5`, any port); any other `Host` gets 403.
  This stops a web page whose domain is re-pointed at 127.0.0.1 from reading or writing a local server. To
  reach a token-less server by a name (`http://bench.lan:8080`), list it in `allowed_hosts` /
  `BENCHLENS_ALLOWED_HOSTS`. With a token every name is served, unless `allowed_hosts` is set, which then
  limits the names in the same way.
- **Upload**: the Upload button in the web UI, or
  `curl -H "Authorization: Bearer $TOKEN" -F file=@results.tar.gz -F tag=round=2 http://host:8080/api/runs`.
  The server's `registry`, when set, tags uploaded runs too (tags of the upload win over it).
- **Reverse proxy**: benchlens speaks plain HTTP; put TLS in front (Caddy, nginx) so the token is not sent in
  the clear, and set a token even when benchlens itself listens on 127.0.0.1 behind the proxy: the proxy makes
  it reachable from outside, and without a token loopback means "writes open". As a safety net, a token-less
  server refuses writes (403) that carry `Forwarded`, `X-Forwarded-For` or `X-Forwarded-Host`, which proxies
  add; a proxy that sends none of them is not detected, so the token is still needed. With a token, pass the
  proxy's public name in `allowed_hosts` if you like, or keep the original `Host` (nginx: `proxy_set_header
  Host $host`). Allow request bodies of at least `max_upload_mb` in the proxy (nginx: `client_max_body_size`).

Details: [docs/api.md](docs/api.md#auth).

## For other tools

The database has three views: `v_runs` (runs with their group), `v_values` (every value that counts, with its
metric name, unit and direction) and `v_stats` (count, mean, min, max per run and metric). SQLite has no
median, so verdicts come from benchlens itself.

## How it stays in step with vmbench

See [docs/contract.md](docs/contract.md): the files benchlens reads, the metric tables, the verdict rule and
the golden tests that check it against vmbench.

## Example data

`tests/fixtures/` and `examples/poc-2026-11/` are real results from a POC of x86 and ARM64 VMs (vmbench
v0.2.1 and v0.2.3). vmbench's hardware.json carries no serial numbers, host names or addresses.

## License

Apache-2.0
