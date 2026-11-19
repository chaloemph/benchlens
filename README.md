# benchlens

Look at, compare and share [vmbench](https://github.com/chaloemph/vmbench) results.

benchlens imports vmbench result folders into a SQLite file and compares machines metric by metric with
**the same rule as `vmbench report compare`**, so its "better / worse / within noise" always agrees with
vmbench's own reports. Compare one run with another, or pool repeated runs of a machine as one group.

> Status: M1 done (core and command line). `benchlens serve` and the API skeleton are in place; the web UI,
> inbox folder and upload come next. See [docs/plan.md](docs/plan.md) and [docs/api.md](docs/api.md).

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
docker run -d --name benchlens -p 8080:8080 -v benchlens-data:/data ghcr.io/chaloemph/benchlens
# import: make the results readable in the container, then run the same image with another command
docker run --rm -v benchlens-data:/data -v "$PWD/results:/results:ro" ghcr.io/chaloemph/benchlens import /results
```

Or with Compose, from a checkout ([docker-compose.yml](docker-compose.yml)):

```bash
docker compose up -d                 # http://localhost:8080
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
```

The database is `benchlens.db` in the current folder unless `--db` or `BENCHLENS_DB` says otherwise.
Importing the same run again, by any route, does not add it twice.

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
benchlens serve --inbox inbox/ --inbox-registry registry.tsv --inbox-tag set=poc
```

- Drop result folders, directories of them, or `.tar` / `.tar.gz` / `.tgz` archives at the top of the inbox.
  An entry is imported once it stayed unchanged for one scan interval (`--interval` / `--inbox-interval`,
  5 s), then moved to `done/`, or to `failed/` with a `.reason.txt`. Upload under a name ending in `.part`,
  `.tmp` or `.partial` and rename when complete; dotfiles and links are ignored.
- `--registry FILE` (`--inbox-registry` for `serve`, env `BENCHLENS_INBOX_REGISTRY` for both) and `--tag`
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
