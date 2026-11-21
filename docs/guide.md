# benchlens user guide

benchlens keeps [vmbench](https://github.com/chaloemph/vmbench) results in one SQLite file and compares
machines metric by metric with the same rule as `vmbench report compare`. This guide covers everything it
does in v0.1.3. For a guided first run see [walkthrough.md](walkthrough.md); for the HTTP API see
[api.md](api.md); for what benchlens reads from vmbench see [contract.md](contract.md).

Every command below was run on the result folders in this repository (`tests/fixtures/`, real vmbench
results from a POC of x86 and ARM64 VMs) and the output is pasted as printed, cut where marked `...`.
The examples use a database `poc.db` in the current folder:

```bash
export BENCHLENS_DB=poc.db       # same as --db poc.db on every command; the default is ./benchlens.db
```

Contents: [Concepts](#concepts) · [Importing](#importing) · [Comparing](#comparing) ·
[Exporting](#exporting) · [Running on a server](#running-on-a-server) · [Docker](#docker-and-compose) ·
[Upgrading](#upgrading) · [Other tools](#reading-the-database-from-other-tools)

## Concepts

**Run.** One vmbench result folder (`meta.json`, `results.tsv`, `status.tsv`, optionally `hardware.json`
and `report.md`). Its id is a hash of label, start time, architecture and vCPU count, so the same run
imported again, by any route, keeps its id and is not stored twice. A run can be named by its id, a unique
id prefix of 4 or more characters, its folder name, or its label when only one run has that label.

**Value.** One line of `results.tsv`: metric id, value, unit and `round` (the repeat number inside the
run; a metric measured three times has rounds 1, 2, 3).

**Metric table.** For every vmbench version, which metrics exist, their unit and their direction (`higher`
is better, `lower` is better, or `none`: shown, never judged). The tables of vmbench 0.2.1 to 0.2.3 are
built in; see [Upgrading](#upgrading) for newer versions.

**Group.** The sample a machine is judged on: all runs of a group are pooled, so three runs with three
repeats each give nine values per metric. By default a run's group is its label without round or rerun
suffixes (`-r2`, `-round2`, `-web`, `-rerun`, `-retry`) plus its CPU, vCPU count and RAM, so the rounds of
one machine pool together and two people's `x86-std` on different hardware do not. Imported without tags,
the fixtures get these groups:

```text
$ benchlens runs
RUN               LABEL                         ARCH      vCPU  VMBENCH  GROUP / TAGS
a96af04543575da1  arm64-high                    aarch64     16  v0.2.1   arm64-high (HiSilicon, 16 vCPU, 31 GB)
180fba1956b130d0  x86-high                      x86_64      16  v0.2.1   x86-high (INTEL(R) XEON(R) GOLD 6526Y, 16 vCPU, 31 GB)
d95fc3263e8efb18  arm64-high-web                aarch64     16  v0.2.3   arm64-high (HiSilicon, 16 vCPU, 31 GB)
74cbd13504ee0072  x86-high-web                  x86_64      16  v0.2.3   x86-high (INTEL(R) XEON(R) GOLD 6526Y, 16 vCPU, 31 GB)
7969870900abc86d  x86-std-r2                    x86_64       2  v0.2.3   x86-std (INTEL(R) XEON(R) GOLD 6526Y, 2 vCPU, 4 GB)
970595d84a8ab297  arm64-high-r2                 aarch64     16  v0.2.3   arm64-high (HiSilicon, 16 vCPU, 31 GB)
d6422680ad835889  x86-high-r2                   x86_64      16  v0.2.3   x86-high (INTEL(R) XEON(R) GOLD 6526Y, 16 vCPU, 31 GB)
```

A `group` tag overrides the automatic group (the registry in `examples/poc-2026-11/` names them
`x86-high`, `arm64-high`, ...).

**Tags.** Any `key=value` on a run. Keys are 1-64 of `A-Z a-z 0-9 _ - .`; values are one line of at most
1024 bytes. Two keys mean something to benchlens:

- `group`: the run's group (above);
- `exclude`: a comma list of stages whose values are not used, e.g. `exclude=web` or `exclude=disk,net`.

Every other tag (`round`, `set`, `owner`, `note` ...) is yours: shown in the runs list, usable as a filter in
the web UI and the API, and usable to split a group (next paragraph). The dot plot of the web UI colours
values by the `round` tag when runs have one.

**REF** (a side of a comparison). One of:

| REF | Means |
|---|---|
| `x86-high-r2`, `d6422680ad835889`, `d642`, `x86-high-r2-20261119-145026` | one run: label, id, id prefix (4+ characters), folder |
| `group:x86-high` | every run of group `x86-high`, pooled |
| `group:x86-high@round=1` | the runs of group `x86-high` whose tag `round` is exactly `1`, pooled |

`group:NAME@KEY=VALUE` takes exactly one `@KEY=VALUE`; the value is compared as it is (no trimming, no case
folding). A selection with no runs is an error that says so:

```text
$ benchlens compare x86-high group:x86-high@round=9
Error: none of the 3 runs of group "x86-high" has tag round="9"
```

A name that matches several runs is refused with a message asking for a run id or `group:NAME`.

**Excluded and failed stages.** A stage with a `FAIL` line in `status.tsv` contributes no values, always;
`benchlens runs` flags such a run with `[failed: STAGE,...]`, and the web UI's Runs view shows the flag too.
A stage listed in the `exclude` tag is left out in the same way, but only because you said so (for
example, a stage that ran with a wrong setting). Both are left out of comparisons, exports, `/api/values`
and the `v_values` view; the stored values stay, so removing the tag brings them back.

**Verdict.** For each metric: median of each side, CV (population standard deviation over the mean, in %),
difference = (other − baseline) / |baseline|. Within noise when |difference| ≤ max(1 %, CV of either side);
otherwise better or worse from the compared side's point of view, taking the direction into account.
sysbench cpu across architectures is shown but not counted (it depends on the compiler). This is exactly
vmbench's `judge()`, checked by golden tests ([contract.md](contract.md#verdict-rule)).

## Importing

Every route goes through the same import code, so a run arrives the same way whichever you use, and a run
that is already there is reported as present instead of being added twice.

What can be imported: a result folder; a directory whose sub-folders are result folders (one level, as
vmbench writes them); a `.tar` or `.tar.gz` (`.tgz`) archive holding one or more result folders. Archives are
read in memory, never extracted: an entry with an absolute path, `..`, a link or a device refuses the whole
archive. Only the files of the [data contract](contract.md#files) are kept.

### Command line: folders and archives

```text
$ benchlens import --dry-run tests/fixtures/v0.2.1
would import  a96af04543575da1  arm64-high-20261118-203447  141 values, vmbench v0.2.1
would import  180fba1956b130d0  x86-high-20261119-033546  141 values, vmbench v0.2.1
0 imported, 0 already present, 2 would import, 0 failed, 0 refused (dry run: nothing written)
```

```text
$ benchlens import tests/fixtures/v0.2.3/x86-std-r2-20261119-103253
present   7969870900abc86d  x86-std-r2-20261119-103253  already imported
0 imported, 1 already present, 0 would import, 0 failed, 0 refused
```

`--tag KEY=VALUE` (repeatable) tags every run of the command; `--json` prints the import reports as JSON.
The exit status is 1 when anything failed or was refused:

```text
$ benchlens import evil.tar          # an archive holding a symbolic link
refused evil.tar: unsafe archive entry link: links, devices and other special entries are not allowed
0 imported, 0 already present, 0 would import, 0 failed, 1 refused
```

### Registry: many runs at once

A registry is a TSV file with a `run` column holding the result folder name and one column per tag; empty
cells are skipped. [examples/poc-2026-11/registry.tsv](../examples/poc-2026-11/registry.tsv) tags the 18 runs
of the POC:

```text
run	group	set	size	round	host	exclude	note
x86-std-20261118-233315	x86-std	poc-x86	std	1	proxmox	web	web failed (CPU pinning), rerun in x86-std-web
...
```

```text
$ benchlens import --registry examples/poc-2026-11/registry.tsv tests/fixtures/v0.2.1 tests/fixtures/v0.2.3
imported  a96af04543575da1  arm64-high-20261118-203447  141 values, vmbench v0.2.1
imported  180fba1956b130d0  x86-high-20261119-033546  141 values, vmbench v0.2.1
imported  970595d84a8ab297  arm64-high-r2-20261119-074530  141 values, vmbench v0.2.3
imported  d95fc3263e8efb18  arm64-high-web-20261119-030942  5 values, vmbench v0.2.3
imported  d6422680ad835889  x86-high-r2-20261119-145026  141 values, vmbench v0.2.3
imported  74cbd13504ee0072  x86-high-web-20261119-101442  5 values, vmbench v0.2.3
imported  7969870900abc86d  x86-std-r2-20261119-103253  141 values, vmbench v0.2.3
7 imported, 0 already present, 0 would import, 0 failed, 0 refused
```

```text
$ benchlens runs
RUN               LABEL                         ARCH      vCPU  VMBENCH  GROUP / TAGS
a96af04543575da1  arm64-high                    aarch64     16  v0.2.1   arm64-high  exclude=web host=proxmox note=web used 8 CPUs only, rerun round=1 set=poc-arm64 size=high
180fba1956b130d0  x86-high                      x86_64      16  v0.2.1   x86-high  exclude=web host=proxmox note=web used 8 CPUs only, rerun in x86-high-web round=1 set=poc-x86 size=high
d95fc3263e8efb18  arm64-high-web                aarch64     16  v0.2.3   arm64-high  host=proxmox note=web rerun v0.2.3 round=1 set=poc-arm64 size=high
74cbd13504ee0072  x86-high-web                  x86_64      16  v0.2.3   x86-high  host=proxmox note=web rerun v0.2.3 round=1 set=poc-x86 size=high
7969870900abc86d  x86-std-r2                    x86_64       2  v0.2.3   x86-std  host=proxmox round=2 set=poc-x86 size=std
970595d84a8ab297  arm64-high-r2                 aarch64     16  v0.2.3   arm64-high  host=proxmox round=2 set=poc-arm64 size=high
d6422680ad835889  x86-high-r2                   x86_64      16  v0.2.3   x86-high  host=proxmox round=2 set=poc-x86 size=high
```

### benchlens-tags.json in the result folder

A result folder may carry `benchlens-tags.json`, `{"key": "value", ...}`, written by you (vmbench does not
write it). It travels with the folder or archive, so the tags arrive by every route. Precedence when a key
is given more than once: **command line (`--tag`, upload `tag` fields) > benchlens-tags.json > registry**.

```bash
mkdir work && cp -r tests/fixtures/v0.2.3/x86-std-r2-20261119-103253 work/
echo '{"group": "poc-x86-std", "owner": "team-a"}' > work/x86-std-r2-20261119-103253/benchlens-tags.json
tar -czf x86-std-r2.tar.gz -C work x86-std-r2-20261119-103253
```

```text
$ benchlens --db fresh.db import x86-std-r2.tar.gz --tag round=2 --tag owner=team-b
imported  7969870900abc86d  x86-std-r2-20261119-103253  141 values, vmbench v0.2.3
1 imported, 0 already present, 0 would import, 0 failed, 0 refused
$ benchlens --db fresh.db runs
RUN               LABEL                         ARCH      vCPU  VMBENCH  GROUP / TAGS
7969870900abc86d  x86-std-r2                    x86_64       2  v0.2.3   poc-x86-std  owner=team-b round=2
```

Importing a run that is already there applies the tags of this import and says so:

```text
$ benchlens --db fresh.db import --json x86-std-r2.tar.gz
[
  {
    "folder": "x86-std-r2-20261119-103253",
    "run_id": "7969870900abc86d",
    "outcome": "already_present",
    "message": "already imported; tags updated"
  }
]
```

### Tags after the import

`benchlens tag RUN KEY=VALUE...` sets tags; an empty value removes one.

```text
$ benchlens tag x86-std-r2 exclude=disk,net note="disk shared with a noisy neighbour"
tags set on 7969870900abc86d
$ benchlens runs | grep x86-std-r2
7969870900abc86d  x86-std-r2                    x86_64       2  v0.2.3   x86-std  exclude=disk,net host=proxmox note=disk shared with a noisy neighbour round=2 set=poc-x86 size=std
$ benchlens compare x86-std-r2 x86-high-r2 | sed -n 1,4p
baseline: x86-std-r2 (1 run)

== x86-high-r2 vs x86-std-r2: better 22, worse 5, within noise 11
   by section (better/worse/within noise): cpu 16/4/11  mem 2/1/0  pg 3/0/0  web 1/0/0
$ benchlens tag x86-std-r2 exclude= note=
tags set on 7969870900abc86d
$ benchlens tag x86-std-r2 'bad key=1'
Error: tag key "bad key": use 1-64 letters, digits, _ - or .
```

The disk and net sections are gone from the tally while `exclude=disk,net` is set. In the web UI, the Runs
view edits tags inline. `benchlens delete RUN` removes a run and its values (its tags are not kept).

### Inbox: drop files into a folder

`benchlens watch` imports whatever is dropped into a folder; `benchlens serve --inbox DIR` does the same in
the background of the server.

- Drop result folders, directories of them, or `.tar` / `.tar.gz` / `.tgz` archives at the top of the inbox.
  An entry is imported once it stayed unchanged for one scan interval (`--interval`, `--inbox-interval` for
  `serve`; 5 s by default), then moved to `done/`, or to `failed/` with a `.reason.txt`.
- Copy under a name ending in `.part`, `.tmp` or `.partial` and rename when complete; dotfiles and links are
  ignored.
- `--registry FILE` (env `BENCHLENS_REGISTRY`; for `serve` also `registry` in benchlens.toml, where it
  applies to uploads too; the old names `--inbox-registry`, `BENCHLENS_INBOX_REGISTRY` and `inbox_registry`
  still work) and `--tag` (`--inbox-tag`) tag inbox imports as `benchlens import` does. The registry is read again whenever it
  changes, so rows can be added while the watcher runs; a registry that no longer parses is reported once
  and the previous one stays in use.

`--once` scans twice, one interval apart, and exits. With a result folder, the archive made above and a
folder that is not a result:

```text
$ benchlens --db inbox.db watch --inbox inbox --interval 1 --once --registry examples/poc-2026-11/registry.tsv --tag set=poc
registry  examples/poc-2026-11/registry.tsv: 18 runs
waiting   not-a-run (imported once it stops changing)
waiting   x86-high-r2-20261119-145026 (imported once it stops changing)
waiting   x86-std-r2.tar.gz (imported once it stops changing)
FAILED    not-a-run: refused: inbox/not-a-run: no vmbench result folder here (a folder with meta.json, or folders that hold one) -> inbox/failed/20261120-051044-not-a-run
done      x86-high-r2-20261119-145026: 1 imported, 0 already present -> inbox/done/20261120-051044-x86-high-r2-20261119-145026
done      x86-std-r2.tar.gz: 1 imported, 0 already present -> inbox/done/20261120-051044-x86-std-r2.tar.gz
$ cat inbox/failed/*.reason.txt
refused: inbox/not-a-run: no vmbench result folder here (a folder with meta.json, or folders that hold one)
$ benchlens --db inbox.db runs
RUN               LABEL                         ARCH      vCPU  VMBENCH  GROUP / TAGS
7969870900abc86d  x86-std-r2                    x86_64       2  v0.2.3   poc-x86-std  host=proxmox owner=team-a round=2 set=poc size=std
d6422680ad835889  x86-high-r2                   x86_64      16  v0.2.3   x86-high  host=proxmox round=2 set=poc size=high
```

(The registry line printed the full path of the registry; shortened here.) The archive's
`benchlens-tags.json` group `poc-x86-std` won over the registry's `x86-std`, as the precedence says.

**One watcher per inbox.** The watcher holds an advisory lock on `<inbox>/.benchlens.lock` (the file names
the holder's pid, host and start time). A second `watch` or `serve --inbox` on the same folder refuses to
start:

```text
$ benchlens watch --inbox inbox --once
Error: inbox is already watched by pid 425278 on <host> (started 20261120-051049 UTC); stop that watcher first (lock file inbox/.benchlens.lock; remove it only if that process is gone)
```

(The host name is replaced by `<host>` here.) A lock left by a watcher that was killed is taken over with a warning. On a network share whose file system
ignores advisory locks, run only one watcher per inbox yourself. `watch` stops on Ctrl-C or SIGTERM after the
import in progress and exits 0; a second Ctrl-C stops at once (an import is one transaction, so an
interrupted entry is imported again on the next run):

```text
watching inbox every 5s (Ctrl-C to stop)
stopping after the import in progress (Ctrl-C again to stop now)
stopped watching inbox
```

### Web upload

The Upload button in the header of the web UI opens a dialog: pick a `.tar` / `.tar.gz`, optionally add tag
rows, and it lists the import report of every folder in the archive. On a server with a token the UI asks
for the token first (see [Running on a server](#running-on-a-server)).

When the server has a registry (`--registry`, `BENCHLENS_REGISTRY` or `registry` in benchlens.toml), it
tags uploaded runs as it tags inbox imports: rows are matched by result folder name, and the registry is
read again at every upload. The usual precedence holds: tags typed in the dialog > the archive's
`benchlens-tags.json` > the registry. A registry that cannot be read or parsed refuses the upload (500)
rather than importing the run untagged.

### HTTP API

`POST /api/runs` takes the same archive as multipart field `file`, plus repeated `tag` fields
([api.md](api.md#post-apiruns-write)). On a server started with a token and the example registry, with the
archive made above (it carries a `benchlens-tags.json`):

```text
$ BENCHLENS_TOKEN=$TOKEN benchlens --db srv.db serve --listen 127.0.0.1:18080 --registry registry.tsv
benchlens 0.1.3 on http://127.0.0.1:18080 (database srv.db)
reads open; writes need the token; uploads up to 256 MiB
Host names served: localhost, IP addresses
registry registry.tsv (inbox imports and uploads)
$ curl -s -F file=@x86-std-r2.tar.gz http://127.0.0.1:18080/api/runs
{"error":"a token is needed: send Authorization: Bearer <token>"}
$ curl -s -H "Authorization: Bearer $TOKEN" -F file=@x86-std-r2.tar.gz -F tag=round=2 http://127.0.0.1:18080/api/runs
[{"folder":"x86-std-r2-20261119-103253","run_id":"7969870900abc86d","outcome":"imported","message":"141 values, vmbench v0.2.3"}]
$ curl -s -H "Authorization: Bearer $TOKEN" -F file=@evil.tar http://127.0.0.1:18080/api/runs
{"error":"unsafe archive entry lnk: links, devices and other special entries are not allowed"}
$ benchlens --db srv.db runs
RUN               LABEL                         ARCH      vCPU  VMBENCH  GROUP / TAGS
7969870900abc86d  x86-std-r2                    x86_64       2  v0.2.3   poc-x86-std  host=proxmox owner=team-a round=2 set=poc-x86 size=std
```

The first answer is 401, the second 200 (each folder's `outcome` is `imported`, `already_present` or
`failed`), the third 400. The uploaded run got `host`, `set` and `size` from the registry, `group` and
`owner` from its `benchlens-tags.json` and `round` from the form. Tags can be changed with
`PATCH /api/runs/{id}/tags` (an empty value removes):

```text
$ curl -s -X PATCH -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' -d '{"owner":""}' http://127.0.0.1:18080/api/runs/x86-std-r2/tags
{"run_id":"7969870900abc86d","folder":"x86-std-r2-20261119-103253","label":"x86-std-r2","group":"poc-x86-std","arch":"x86_64","nproc":2,"ram_gb":4.0,"cpu_model":"INTEL(R) XEON(R) GOLD 6526Y","vmbench_version":"v0.2.3","start_utc":"2026-11-19T03:32:55Z","source":"upload","catalog_known":true,"failed_stages":[],"tags":{"group":"poc-x86-std","host":"proxmox","round":"2","set":"poc-x86","size":"std"},"values":141,"kept_files":["benchlens-tags.json","hardware.json","meta.json","results.tsv","status.tsv"]}
```

`kept_files` lists the files the run kept (a `report.md` when vmbench wrote one); `GET /api/runs` carries it
for every run.

Each run records how it arrived in `source`: `cli`, `inbox` or `upload`.

## Comparing

### Command line

`benchlens compare BASE OTHER [OTHER...]` with any REFs. Groups against groups:

```text
$ benchlens compare group:x86-high group:arm64-high
baseline: x86-high (3 runs)

== arm64-high vs x86-high: better 23, worse 37, within noise 5
   by section (better/worse/within noise): cpu 4/25/0  disk 16/2/4  mem 0/2/1  net 0/7/0  pg 3/0/0  web 0/1/0
   metric                                           x86-high   arm64-high     diff  verdict
   7z rating · all vCPU ↑ (MIPS)                      80,940       66,346   -18.0%  worse
   cpubench crypto/aes-gcm · 1 thread ↑ (MB/s)         6,582        3,729   -43.4%  worse
   cpubench crypto/aes-gcm · all vCPU ↑ (MB/s)        91,723       56,616   -38.3%  worse !
...
   pgbench tpcb-like tps ↑ (TPS)                       4,820       11,105  +130.4%  better
   cpubench web mix max RPS @ SLO (loopback) ↑        31,990       15,997   -50.0%  worse !

↑ higher is better, ↓ lower is better; * measured once (noise unknown); ! CV of 5% or more
note: x86-high and arm64-high are different architectures: sysbench cpu is shown but not counted (it depends on the compiler)
```

Reading it: medians of each side, the difference from the baseline, and the verdict from the compared side's
point of view. `!` marks a metric whose run-to-run variation is 5 % or more on either side, `*` one measured
only once (no noise estimate, so any difference over 1 % counts). The x86-high group pools x86-high (round 1,
with its web stage excluded), its web rerun x86-high-web, and x86-high-r2.

Rounds of one machine against each other, with group subsets and `--section` (rows of one section only; the
tallies stay complete):

```text
$ benchlens compare group:x86-high@round=1 group:x86-high@round=2 --section mem
baseline: x86-high · round=1 (2 runs)

== x86-high · round=2 vs x86-high · round=1: better 13, worse 18, within noise 36
   by section (better/worse/within noise): cpu 0/5/26  disk 8/7/7  mem 1/2/0  net 3/3/1  pg 1/1/1  web 0/0/1
   metric                                       x86-high · round=1 x86-high · round=2     diff  verdict
   lat_mem_rd · 1 thread ↓ (ns)                                147                135    -8.5%  better
   stream copy · all vCPU ↑ (MB/s)                         189,348            176,347    -6.9%  worse !
   stream triad · all vCPU ↑ (MB/s)                        164,316            147,856   -10.0%  worse

↑ higher is better, ↓ lower is better; * measured once (noise unknown); ! CV of 5% or more
```

One run against another:

```text
$ benchlens compare x86-std-r2 x86-high-r2 --section pg
baseline: x86-std-r2 (1 run)

== x86-high-r2 vs x86-std-r2: better 39, worse 10, within noise 18
   by section (better/worse/within noise): cpu 16/4/11  disk 11/4/7  mem 2/1/0  net 6/1/0  pg 3/0/0  web 1/0/0
   metric                                         x86-std-r2  x86-high-r2     diff  verdict
   pgbench single-session latency ↓ (ms)               1.475        1.270   -13.9%  better *
   tps stability 100*(1-cv) ↑ (%)                       85.8         86.9    +1.2%  better *
   pgbench tpcb-like tps ↑ (TPS)                       1,733        4,837  +179.0%  better *

↑ higher is better, ↓ lower is better; * measured once (noise unknown); ! CV of 5% or more
```

More than one OTHER gives one block per side, each against the same baseline. `--json` prints the
`Comparison` of [api.md](api.md#get-apicomparebaserefotherrefotherrefsectioncpu) (`base`, `others`, `rows`,
`tallies`, `warnings`), the same numbers the web UI shows:

```text
$ benchlens compare d642 group:arm64-high --json
{
  "base": {
    "reference": "d642",
    "label": "x86-high-r2",
    "run_ids": [
      "d6422680ad835889"
    ],
...
  "rows": [
    {
      "other": 0,
      "id": "cpu.7z.rating.max",
      "name": "7z rating · all vCPU",
      "section": "cpu",
      "unit": "MIPS",
      "direction": "higher",
      "base": 80154.0,
      "value": 66346.5,
      "cv_base": 0.49581319153273695,
      "cv_value": 1.4482838610465507,
      "n_base": 3,
      "n_value": 6,
      "diff": -17.226214537016244,
...
```

Piped into a reader that stops early, every command but `serve` ends quietly, killed by SIGPIPE like other
Unix tools; the shell reports status 141 (with `set -o pipefail` that counts as a failure):

```text
$ benchlens --db fx.db export csv group:x86-high group:arm64-high | head -2; echo "status ${PIPESTATUS[0]}"
side,run_id,label,id,name,section,unit,direction,round,value
group:x86-high,180fba1956b130d0,x86-high,cpu.7z.rating.max,7z rating · all vCPU,cpu,MIPS,higher,1,81684
status 141
```

### Web UI

`benchlens serve` and open <http://127.0.0.1:8080>. Four views, all state in the URL (any page can be shared
as a link):

- **Compare**: baseline picker, "compare with" checkboxes for groups and single runs, quick picks derived from
  the data (such as "x86 vs ARM64 · high" or "Round 1 vs 2 · x86-high"), a takeaway sentence, the tally table
  per section, and a heatmap of every metric (key metrics or one section; sysbench across architectures
  hatched and not counted). Choosing a metric name opens the Metric view. **Compare with: a group split by
  tag** picks a group, a tag key and some of its values, and adds one side `group:NAME@KEY=VALUE` per value
  (the runs of the group with that value, pooled), optionally with the first value as the baseline; for
  example `round=1` against `round=2` of `x86-high`. Sides added this way are listed under it and can be
  unticked there.
- **Metric**: a dot plot of every raw value of one metric per side, coloured by the run's `round` tag, with
  the median tick, and the statistics table.
- **Runs**: search and filters (text, architecture, group, tag), inline tag editing, delete with
  confirmation, a link to the run's `report.md` when it kept one (a click shows it as plain text in a dialog,
  fetched with the token, so it works on a private server too; open it in a new tab for the raw file),
  flags for failed stages, excluded stages and missing metric tables.
- **Hardware**: `hardware.json` of the selected runs side by side, differing rows highlighted.

The URL keys are `view`, `base`, `other` (repeated), `section`, `metric`, `run`, and the runs filters `q`,
`arch`, `group`, `tag`; for example
`/?view=compare&base=group:x86-high&other=group:arm64-high&section=key`. The UI computes no numbers itself:
everything comes from `/api/compare` and equals `benchlens compare --json`. Screenshots are in
[walkthrough.md](walkthrough.md#5-the-web-ui).

`group:NAME@KEY=VALUE` sides come from the split control above, the "Round 1 vs 2 · ..." quick picks, or a
link with `base=` / `other=` in the URL.

## Exporting

To share a comparison with someone who has no benchlens:

```text
$ benchlens export html --base group:x86-high --other group:arm64-high -o report.html --title "POC high: x86 vs ARM64"
wrote report.html (752 KB): 2 sides, 6 runs, 568 values
```

The file opens from disk with no server and no network (about 230 KB gzipped for these two groups). It
inlines the web UI's compare, metric and hardware views, d3, the WebAssembly build of the verdict rule and the
values of the exported sides. In the page the reader can:

- pick any exported side, or any single run of one, as the baseline or a compared side (a group exported as a
  side can be split into its runs, not into runs of other groups);
- see the tally, heatmap, dot plots, statistics and hardware tables, as in the served UI;
- trust that the numbers equal `benchlens compare`: they come from the same rule, compiled to WebAssembly.

It cannot import, tag or reach other data. A Content-Security-Policy in the file allows no network access.
The Export HTML button of the Compare view (and `GET /api/export.html`) produces the same file for the
shown sides.

Values for a spreadsheet or a script, one row per raw value:

```text
$ benchlens export csv group:x86-high group:arm64-high -o values.csv
$ head -4 values.csv
side,run_id,label,id,name,section,unit,direction,round,value
group:x86-high,180fba1956b130d0,x86-high,cpu.7z.rating.max,7z rating · all vCPU,cpu,MIPS,higher,1,81684
group:x86-high,180fba1956b130d0,x86-high,cpu.7z.rating.max,7z rating · all vCPU,cpu,MIPS,higher,2,81853
group:x86-high,180fba1956b130d0,x86-high,cpu.7z.rating.max,7z rating · all vCPU,cpu,MIPS,higher,3,82158
```

`side` is the REF as given, `label` the run's label, `round` the repeat inside the run, `direction` empty for a
vmbench version without a metric table. Text starting with `= + - @` gets a leading `'` so spreadsheets do not
run it as a formula. Excluded and failed stages are left out, as in comparisons.

`export json` writes `{benchlens, exported_at, source, sides}`; each side holds `side`, `stats` (per metric:
`id`, `name`, `section`, `unit`, `direction`, `tool`, `n`, `median`, `cv`, `min`, `max`), `unjudged` and
`values`. Without `-o`, `json` and `csv` write to stdout. One entry of `stats` of the first side of
`benchlens export json group:x86-high@round=2 x86-std-r2`:

```json
{
 "id": "cpu.7z.rating.max",
 "name": "7z rating · all vCPU",
 "section": "cpu",
 "unit": "MIPS",
 "direction": "higher",
 "tool": "7-Zip",
 "n": 3,
 "median": 80154.0,
 "cv": 0.49581319153273695,
 "min": 79336.0,
 "max": 80197.0
}
```

## Running on a server

`benchlens serve` listens on `127.0.0.1:8080` by default. Its settings come from flags, then `BENCHLENS_*`
variables, then an optional `benchlens.toml` (`--config FILE` / `BENCHLENS_CONFIG`, else `./benchlens.toml`
when present), then the defaults. The [README](../README.md#running-on-a-server) lists every key.

| Setting | Flag | Variable | benchlens.toml | Default |
|---|---|---|---|---|
| database | `--db` | `BENCHLENS_DB` | `db` | `benchlens.db` |
| address | `--listen` | `BENCHLENS_LISTEN` | `listen` | `127.0.0.1:8080` |
| token | (none) | `BENCHLENS_TOKEN` | `token` | none |
| reads need the token | `--read-public false` | `BENCHLENS_READ_PUBLIC` | `read_public` | `true` |
| largest upload | `--max-upload-mb` | `BENCHLENS_MAX_UPLOAD_MB` | `max_upload_mb` | 256 |
| inbox | `--inbox` | `BENCHLENS_INBOX` | `inbox` | off |
| inbox scan interval | `--inbox-interval` | `BENCHLENS_INBOX_INTERVAL` | `inbox_interval` | 5 |
| registry (inbox imports and uploads) | `--registry` (old: `--inbox-registry`) | `BENCHLENS_REGISTRY` (old: `BENCHLENS_INBOX_REGISTRY`) | `registry` (old: `inbox_registry`) | none |
| inbox tags | `--inbox-tag K=V` | | `inbox_tags = { k = "v" }` | none |
| extra Host names served | | `BENCHLENS_ALLOWED_HOSTS` (comma list) | `allowed_hosts = ["a", "b"]` | `BENCHLENS_DEFAULT_ALLOWED_HOSTS` (comma list; the Docker image sets `localhost`), else none |

For the extra Host names there is one more source with the lowest priority, `BENCHLENS_DEFAULT_ALLOWED_HOSTS`
(since 0.1.3): it is used only when neither `BENCHLENS_ALLOWED_HOSTS` nor `allowed_hosts` in the file gives
names. The Docker image sets it to `localhost`, so a mounted `benchlens.toml` replaces the image default. An
empty list counts as not set; `BENCHLENS_ALLOWED_HOSTS=` (set, but empty or only blanks and commas) also drops
the default list.

### Who may read and write

| Configuration | Reads | Writes (upload, tags, delete) |
|---|---|---|
| no token, loopback address | open | open (local use) |
| no token, any other address | open | refused, 403 |
| token, `read_public = true` | open | `Authorization: Bearer <token>`, else 401 |
| token, `read_public = false` | token, else 401 (`/api/health` and the page itself stay open) | token |

Without a token, a write that a browser marks as coming from a page of another origin is also refused (403),
so a web page you visit cannot upload to your local server. The web UI asks for the token once per tab when
the server answers 401.

**Host names (DNS rebinding).** On a loopback address (`127.0.0.1`, `::1`, also in the IPv4-mapped form
`[::ffff:127.0.0.1]`), with or without a token, and on
any address without a token, the server answers only requests addressed to `localhost` or an IP address
(`127.0.0.1`, `[::1]`, `192.168.1.5`, any port), and the names in `allowed_hosts`; any other `Host` gets 403,
for the web UI files as well as the API. A web page whose domain is re-pointed at 127.0.0.1 sends its own
domain as `Host`, so it cannot read or write a local server, not even the data that `read_public = true`
makes public on a server with a token; an IP address cannot be re-pointed, and the port is not checked so
port mappings keep working. On a non-loopback address with a token every name is served, unless
`allowed_hosts` is set, which then limits the names in the same way (the Docker image sets
`BENCHLENS_DEFAULT_ALLOWED_HOSTS=localhost` for this, see [Host names in a container](#host-names-in-a-container)).
The 403 names the exact setting that would serve the refused name, given where the current list came from
(without showing the names already configured):

```text
$ benchlens --db local.db serve --listen 127.0.0.1:18084
benchlens 0.1.3 on http://127.0.0.1:18084 (database local.db)
reads open; writes open (loopback, no token); uploads up to 256 MiB
Host names served: localhost, IP addresses
$ curl -s http://localhost:18084/api/health
{"benchlens":"0.1.3","db_layout":2,"runs":0}
$ curl -s -H 'Host: evil.example:18084' http://127.0.0.1:18084/api/health
{"error":"Host \"evil.example:18084\" refused: on a loopback address only localhost, IP addresses and allowed_hosts are served (DNS rebinding guard)","detail":"to serve this name (for example behind a reverse proxy on this machine that passes its public name as Host), set allowed_hosts = [\"evil.example\"] in benchlens.toml or BENCHLENS_ALLOWED_HOSTS=evil.example"}
```

This applies with a token too (since 0.1.1): if you open a loopback server by a name, such as an
`/etc/hosts` alias (`http://bench.local:8080`) or a name that a tunnel or proxy passes on as `Host`, that
name gets 403 until it is in `allowed_hosts`. `http://localhost:8080` and `http://127.0.0.1:8080` always
work, also through `ssh -L 8080:127.0.0.1:8080` (the browser sends `Host: localhost:8080`).

To reach a loopback server by a name, list it:

```text
$ BENCHLENS_ALLOWED_HOSTS=bench.lan benchlens --db local.db serve --listen 127.0.0.1:18084
benchlens 0.1.3 on http://127.0.0.1:18084 (database local.db)
reads open; writes open (loopback, no token); uploads up to 256 MiB
Host names served: localhost, IP addresses, bench.lan (from BENCHLENS_ALLOWED_HOSTS)
$ curl -s -H 'Host: bench.lan:18084' http://127.0.0.1:18084/api/health
{"benchlens":"0.1.3","db_layout":2,"runs":0}
$ curl -s -H 'Host: evil.example:18084' http://127.0.0.1:18084/api/health
{"error":"Host \"evil.example:18084\" refused: on a loopback address only localhost, IP addresses and allowed_hosts are served (DNS rebinding guard)","detail":"to serve this name (for example behind a reverse proxy on this machine that passes its public name as Host), add it to BENCHLENS_ALLOWED_HOSTS: BENCHLENS_ALLOWED_HOSTS=evil.example (keep the names already listed; this variable wins over allowed_hosts in benchlens.toml)"}
```

The list here comes from `BENCHLENS_ALLOWED_HOSTS`, so the 403 names only that variable: `allowed_hosts` in
a config file would have no effect. With the list in the file it names the file key instead.

The start-up lines say which case applies:

```text
$ BENCHLENS_TOKEN=$TOKEN benchlens --db srv.db serve --listen 127.0.0.1:18080
benchlens 0.1.3 on http://127.0.0.1:18080 (database srv.db)
reads open; writes need the token; uploads up to 256 MiB
Host names served: localhost, IP addresses
```

```text
$ BENCHLENS_TOKEN=$TOKEN benchlens --db srv.db serve --listen 0.0.0.0:18085
benchlens 0.1.3 on http://0.0.0.0:18085 (database srv.db)
reads open; writes need the token; uploads up to 256 MiB
Host names served: any
warning: listening on 0.0.0.0:18085 with no Host check: any Host name is served, so a DNS rebinding web page can read the public data (writes still need the token); set allowed_hosts in benchlens.toml or BENCHLENS_ALLOWED_HOSTS=name to turn the check on, or read_public = false to keep reads behind the token
```

With a token on a non-loopback address and no `allowed_hosts`, the Host check is off: a DNS rebinding page in
a browser on the same network can read what `read_public = true` makes public (it never sees the token, so it
cannot write). The server warns about it at start-up (since 0.1.4); the warning is quiet once
`allowed_hosts` / `BENCHLENS_ALLOWED_HOSTS` lists a name, with `read_public = false` (then only
`/api/health` and the page itself are open without the token), and on loopback addresses (checked there
anyway).

```text
$ benchlens --db srv.db serve --listen 0.0.0.0:18081
benchlens 0.1.3 on http://0.0.0.0:18081 (database srv.db)
reads open; writes refused (no token); uploads up to 256 MiB
Host names served: localhost, IP addresses
warning: listening on 0.0.0.0:18081 without a token: anyone who can reach it can read, and writes (upload, tags, delete) are refused; set BENCHLENS_TOKEN to allow them
$ curl -s -X DELETE http://127.0.0.1:18081/api/runs/x86-std-r2
{"error":"writes are turned off: this server listens on a non-loopback address without a token","detail":"start it with BENCHLENS_TOKEN (or `token` in benchlens.toml) to allow upload, tags and delete"}
```

The token has no command-line flag (command lines are visible to other users of the machine), must be at
least 16 characters and is never printed. Misconfigurations stop the server before it listens:

```text
$ benchlens --db srv.db serve --listen 127.0.0.1:18082 --read-public false
Error: read_public = false needs a token (BENCHLENS_TOKEN or `token` in benchlens.toml)
$ BENCHLENS_TOKEN=short benchlens --db srv.db serve --listen 127.0.0.1:18082
Error: the token is too short: use at least 16 characters (e.g. `openssl rand -hex 32`)
```

A private server with an inbox, from a `benchlens.toml` (the token is a placeholder here):

```toml
db = "/srv/benchlens/srv.db"
listen = "127.0.0.1:18083"
token = "<output of openssl rand -hex 32>"
read_public = false
inbox = "/srv/benchlens/inbox2"
registry = "/srv/benchlens/registry.tsv"
inbox_tags = { set = "poc" }
```

```text
$ benchlens serve                      # in the folder holding benchlens.toml
watching inbox /srv/benchlens/inbox2 every 5s
benchlens 0.1.3 on http://127.0.0.1:18083 (database /srv/benchlens/srv.db)
reads need the token; writes need the token; uploads up to 256 MiB
Host names served: localhost, IP addresses
registry /srv/benchlens/registry.tsv (inbox imports and uploads)
inbox: registry  /srv/benchlens/registry.tsv: 18 runs
inbox: waiting   arm64-high-r2-20261119-074530 (imported once it stops changing)
inbox: done      arm64-high-r2-20261119-074530: 1 imported, 0 already present -> /srv/benchlens/inbox2/done/20261120-054511-arm64-high-r2-20261119-074530
$ curl -s http://127.0.0.1:18083/api/runs
{"error":"a token is needed: send Authorization: Bearer <token>"}
$ curl -s http://127.0.0.1:18083/api/health
{"benchlens":"0.1.3","db_layout":2,"runs":1}
```

(Paths were in a scratch folder when this ran; shown here as `/srv/benchlens`.) `benchlens serve` stops
cleanly on Ctrl-C or SIGTERM.

### Behind a reverse proxy

benchlens speaks plain HTTP. Put TLS in front (Caddy, nginx) so the token is not sent in the clear, and
**set a token even when benchlens listens on 127.0.0.1 behind the proxy**: the proxy makes it reachable
from outside, and on loopback without a token writes are open. Allow request bodies of at least
`max_upload_mb` in the proxy (nginx: `client_max_body_size`). For example, a minimal Caddyfile (not part of the test runs of this guide):

```text
bench.example.org {
    reverse_proxy 127.0.0.1:8080
}
```

As a safety net, a server without a token refuses writes that carry `Forwarded`, `X-Forwarded-For` or
`X-Forwarded-Host`, which proxies usually add (reads through the proxy still work):

```text
$ curl -s -X DELETE -H 'X-Forwarded-For: 203.0.113.7' http://127.0.0.1:18084/api/runs/x86-std-r2
{"error":"write refused: it came through a reverse proxy and this server has no token","detail":"the request carries x-forwarded-for; set a token (BENCHLENS_TOKEN or `token` in benchlens.toml) when serving behind a proxy"}
```

A proxy that adds none of these headers is not detected (nginx's plain `proxy_pass` adds none), so the token
is still what protects the server.

**The proxy's Host.** benchlens on 127.0.0.1 checks the `Host` also with a token. A proxy that passes the
public name as `Host` (Caddy does by default; nginx with `proxy_set_header Host $host`) gets 403 until that
name is in `allowed_hosts`; nginx's default (`Host` is the upstream address, `127.0.0.1:8080`) is an IP
address and is served. The 403 says what to set:

```text
$ BENCHLENS_TOKEN=$TOKEN benchlens --db srv.db serve --listen 127.0.0.1:18080
$ curl -s -H 'Host: bench.example.org' http://127.0.0.1:18080/api/health
{"error":"Host \"bench.example.org\" refused: on a loopback address only localhost, IP addresses and allowed_hosts are served (DNS rebinding guard)","detail":"to serve this name (for example behind a reverse proxy on this machine that passes its public name as Host), set allowed_hosts = [\"bench.example.org\"] in benchlens.toml or BENCHLENS_ALLOWED_HOSTS=bench.example.org"}
$ BENCHLENS_TOKEN=$TOKEN BENCHLENS_ALLOWED_HOSTS=bench.example.org benchlens --db srv.db serve --listen 127.0.0.1:18080
benchlens 0.1.3 on http://127.0.0.1:18080 (database srv.db)
reads open; writes need the token; uploads up to 256 MiB
Host names served: localhost, IP addresses, bench.example.org (from BENCHLENS_ALLOWED_HOSTS)
$ curl -s -H 'Host: bench.example.org' http://127.0.0.1:18080/api/health
{"benchlens":"0.1.3","db_layout":2,"runs":0}
```

(The start-up lines of the first server are left out.) On a non-loopback address with a token every `Host`
is served unless `allowed_hosts` is set.

What the server does and does not protect against: [SECURITY.md](../SECURITY.md).

## Docker and Compose

The image `ghcr.io/chaloemph/benchlens` (linux/amd64 and linux/arm64) is a static binary on distroless,
running `benchlens serve` as uid 65532 on `0.0.0.0:8080` with the database at `/data/benchlens.db` (a
volume). Because `0.0.0.0` is not loopback, **without `BENCHLENS_TOKEN` the container is read-only**. The
image also sets `BENCHLENS_DEFAULT_ALLOWED_HOSTS=localhost`, so the container answers only `localhost` and IP
addresses as `Host`, with or without a token (see [Host names in a container](#host-names-in-a-container)).

The commands below ran against an image built from this checkout (`docker build -t benchlens:h-docs .`);
with the published image, use `ghcr.io/chaloemph/benchlens` instead of `benchlens:h-docs`.

```text
$ docker run --rm -v benchlens-data:/data -v "$PWD/tests/fixtures:/results:ro" benchlens:h-docs import /results/v0.2.1 /results/v0.2.3
imported  a96af04543575da1  arm64-high-20261118-203447  141 values, vmbench v0.2.1
...
imported  7969870900abc86d  x86-std-r2-20261119-103253  141 values, vmbench v0.2.3
7 imported, 0 already present, 0 would import, 0 failed, 0 refused
$ docker run -d --name benchlens -p 8080:8080 -v benchlens-data:/data -e BENCHLENS_TOKEN="$(openssl rand -hex 32)" benchlens:h-docs
<container id>
$ docker logs benchlens
benchlens 0.1.3 on http://0.0.0.0:8080 (database /data/benchlens.db)
reads open; writes need the token; uploads up to 256 MiB
Host names served: localhost, IP addresses (from BENCHLENS_DEFAULT_ALLOWED_HOSTS)
$ curl -s http://localhost:8080/api/health
{"benchlens":"0.1.3","db_layout":2,"runs":7}
$ docker stop benchlens               # SIGTERM: the server stops cleanly
```

(Run with another container name, volume and host port, so as not to disturb a running instance; the output
is otherwise as printed.) The import runs the same image with another command; mount the results read-only.
Keep the token somewhere: the web UI and `curl` need it for upload, tag editing and delete. Started without
`BENCHLENS_TOKEN`, the container says so:

```text
benchlens 0.1.3 on http://0.0.0.0:8080 (database /data/benchlens.db)
reads open; writes refused (no token); uploads up to 256 MiB
Host names served: localhost, IP addresses (from BENCHLENS_DEFAULT_ALLOWED_HOSTS)
warning: listening on 0.0.0.0:8080 without a token: anyone who can reach it can read, and writes (upload, tags, delete) are refused; set BENCHLENS_TOKEN to allow them
```

### Host names in a container

The container listens on `0.0.0.0`, so the loopback Host check (see
[Who may read and write](#who-may-read-and-write)) would not apply to it, and with a token it would answer any
`Host`: a DNS rebinding page could then read what `read_public = true` makes public through the published
port, even one published only as `127.0.0.1:8080:8080`. The image therefore turns the check on (since
0.1.2): `http://localhost:8080` and the container by IP address (`http://192.168.1.5:8080`,
`http://127.0.0.1:8080`) work, any other name gets 403 and the error says what to set. It does so with
`BENCHLENS_DEFAULT_ALLOWED_HOSTS=localhost` (since 0.1.3; 0.1.2 set `BENCHLENS_ALLOWED_HOSTS=localhost`), a
default with the lowest priority: names you give in `BENCHLENS_ALLOWED_HOSTS` or in `allowed_hosts` of a
mounted `benchlens.toml` replace it. With the container above (token set; `192.168.1.5` stands for the Docker
host's address; the outputs in this section were captured with an image built from this checkout, and the
container and volume names were edited to match the commands):

```text
$ curl -s http://192.168.1.5:8080/api/health
{"benchlens":"0.1.3","db_layout":2,"runs":7}
$ curl -s -H 'Host: bench.lan:8080' http://localhost:8080/api/health
{"error":"Host \"bench.lan:8080\" refused: it is not in allowed_hosts","detail":"to serve this name (for example behind a reverse proxy on this machine that passes its public name as Host), set allowed_hosts = [\"bench.lan\"] in benchlens.toml or BENCHLENS_ALLOWED_HOSTS=bench.lan; either replaces the default list (BENCHLENS_DEFAULT_ALLOWED_HOSTS); localhost and IP addresses stay served"}
```

To open the container by a DNS name (a LAN name, or a reverse proxy that passes its public name as `Host`),
list the names in either place. Your list **replaces** the image default rather than adding to it; nothing
is lost, because `localhost` and IP addresses are always served once the check is on. With a mounted
`benchlens.toml` holding `allowed_hosts = ["bench.lan"]`:

```text
$ docker run -d --name benchlens -p 8080:8080 -v benchlens-data:/data -v "$PWD/benchlens.toml:/data/benchlens.toml:ro" -e BENCHLENS_TOKEN="$TOKEN" benchlens:h-docs
$ docker logs benchlens
benchlens 0.1.3 on http://0.0.0.0:8080 (database /data/benchlens.db)
reads open; writes need the token; uploads up to 256 MiB
Host names served: localhost, IP addresses, bench.lan (from allowed_hosts in the config file)
$ curl -s -H 'Host: bench.lan:8080' http://localhost:8080/api/health
{"benchlens":"0.1.3","db_layout":2,"runs":7}
$ curl -s -H 'Host: evil.example:8080' http://localhost:8080/api/health
{"error":"Host \"evil.example:8080\" refused: it is not in allowed_hosts","detail":"to serve this name (for example behind a reverse proxy on this machine that passes its public name as Host), add it to allowed_hosts in benchlens.toml: allowed_hosts = [\"evil.example\"] (keep the names already listed)"}
```

Or with the variable, without a file:

```text
$ docker run -d --name benchlens -p 8080:8080 -v benchlens-data:/data -e BENCHLENS_TOKEN="$TOKEN" -e BENCHLENS_ALLOWED_HOSTS=bench.lan benchlens:h-docs
$ docker logs benchlens
benchlens 0.1.3 on http://0.0.0.0:8080 (database /data/benchlens.db)
reads open; writes need the token; uploads up to 256 MiB
Host names served: localhost, IP addresses, bench.lan (from BENCHLENS_ALLOWED_HOSTS)
$ curl -s -H 'Host: bench.lan:8080' http://localhost:8080/api/health
{"benchlens":"0.1.3","db_layout":2,"runs":7}
$ curl -s http://localhost:8080/api/health
{"benchlens":"0.1.3","db_layout":2,"runs":7}
```

In compose, set `BENCHLENS_ALLOWED_HOSTS: bench.example.org` under `environment` (there is a commented line
for it). A reverse proxy in another container that reaches benchlens by its service or container name (nginx
`proxy_pass http://benchlens:8080;` passes `benchlens:8080` as `Host` unless told otherwise) needs that name
listed too, or `proxy_set_header Host $host;` and the public name listed.

`BENCHLENS_ALLOWED_HOSTS` wins over `allowed_hosts` in a mounted `benchlens.toml` (the environment beats the
file), and the server says so at start-up when that hides a different list (here
`-e BENCHLENS_ALLOWED_HOSTS=other.lan` with the file above); the 403 then names only the variable:

```text
$ docker run -d --name benchlens -p 8080:8080 -v benchlens-data:/data -v "$PWD/benchlens.toml:/data/benchlens.toml:ro" -e BENCHLENS_TOKEN="$TOKEN" -e BENCHLENS_ALLOWED_HOSTS=other.lan benchlens:h-docs
$ docker logs benchlens
benchlens 0.1.3 on http://0.0.0.0:8080 (database /data/benchlens.db)
reads open; writes need the token; uploads up to 256 MiB
Host names served: localhost, IP addresses, other.lan (from BENCHLENS_ALLOWED_HOSTS)
warning: allowed_hosts in the config file (bench.lan) is ignored: BENCHLENS_ALLOWED_HOSTS (other.lan) wins over the file; put the names in BENCHLENS_ALLOWED_HOSTS, or remove it to use the file's list
$ curl -s -H 'Host: bench.lan:8080' http://localhost:8080/api/health
{"error":"Host \"bench.lan:8080\" refused: it is not in allowed_hosts","detail":"to serve this name (for example behind a reverse proxy on this machine that passes its public name as Host), add it to BENCHLENS_ALLOWED_HOSTS: BENCHLENS_ALLOWED_HOSTS=bench.lan (keep the names already listed; this variable wins over allowed_hosts in benchlens.toml)"}
```

**Opting out** (not recommended for a container that a browser can reach): `-e BENCHLENS_ALLOWED_HOSTS=` (an
empty value, or one with only blanks or commas) counts as not set and also drops the image default, as in
0.1.2. With no `allowed_hosts` in a file and a token the container then serves every `Host`
(`Host names served: any`) and says so in `docker logs` (since 0.1.4); with the file's list it serves that
list, without the warning:

```text
$ docker run -d --name benchlens -p 8080:8080 -v benchlens-data:/data -e BENCHLENS_TOKEN="$TOKEN" -e BENCHLENS_ALLOWED_HOSTS= benchlens:h-docs
$ docker logs benchlens
benchlens 0.1.3 on http://0.0.0.0:8080 (database /data/benchlens.db)
reads open; writes need the token; uploads up to 256 MiB
Host names served: any
warning: listening on 0.0.0.0:8080 with no Host check: any Host name is served, so a DNS rebinding web page can read the public data (writes still need the token); set allowed_hosts in benchlens.toml or BENCHLENS_ALLOWED_HOSTS=name to turn the check on, or read_public = false to keep reads behind the token
```

**Behaviour change in 0.1.2**: before, a container with a token answered any name. If you open yours by a
name (a LAN name, a reverse proxy's public name, a proxy container's upstream name), it now answers 403
`Host "..." refused: it is not in allowed_hosts`: add that name with `-e BENCHLENS_ALLOWED_HOSTS=name` (or in
compose, or in `allowed_hosts` of a mounted `benchlens.toml` since 0.1.3) when you upgrade. Without a token the
check was already on, as it still is.

**Change in 0.1.3**: `allowed_hosts` in a mounted `benchlens.toml` now takes effect without
`-e BENCHLENS_ALLOWED_HOSTS=`; in 0.1.2 the image default hid it (with a start-up warning). Anything set with
`-e BENCHLENS_ALLOWED_HOSTS=...`, empty or not, behaves as in 0.1.2.

Compose, from a checkout ([docker-compose.yml](../docker-compose.yml)):

```bash
BENCHLENS_TOKEN=$(openssl rand -hex 32) docker compose up -d     # http://localhost:8080
docker compose run --rm -v "$PWD/results:/results:ro" benchlens import /results
```

```text
$ docker compose run --rm -v "$PWD/results:/results:ro" benchlens import /results
imported  7969870900abc86d  x86-std-r2-20261119-103253  141 values, vmbench v0.2.3
1 imported, 0 already present, 0 would import, 0 failed, 0 refused
```

(Checked with `docker compose config` and the `run` line above; `up -d` was not run for this guide because
port 8080 was taken on the test machine. `docker compose build` builds the image locally instead of pulling
it.)

The compose file has commented lines for an inbox (`BENCHLENS_INBOX=/data/inbox`, a mounted `./inbox`
writable by uid 65532) and `BENCHLENS_READ_PUBLIC: "false"`.

### Grafana

The compose profile `grafana` adds Grafana 13 with the SQLite datasource on the benchlens volume, mounted
read-only and opened with `mode=ro`, and three dashboards: **Metric over time** (one metric, a series per
group, a point per run), **Group vs group** (mean, min, max and n per metric and group) and **Runs** (tags,
vmbench version, failed stages). Grafana shows means and ranges, since SQLite has no median; whether a
difference is better, worse or within noise is decided by benchlens only, and every dashboard links back to
the benchlens UI.

```bash
BENCHLENS_TOKEN=$(openssl rand -hex 32) GRAFANA_ADMIN_PASSWORD=change-me docker compose --profile grafana up -d
# http://localhost:3000 (Grafana) and http://localhost:8080 (benchlens)
```

Log in as `admin` / `$GRAFANA_ADMIN_PASSWORD`; anonymous access is off, so without a login Grafana shows
only its login page. **The admin password defaults to `admin`: change it** by setting
`GRAFANA_ADMIN_PASSWORD` before the first start (Grafana keeps the first password in its volume). On your own
machine you can opt in to anonymous viewers:

```bash
GRAFANA_ANONYMOUS=true GRAFANA_ADMIN_PASSWORD=change-me docker compose --profile grafana up -d
```

Grafana reads the database file, not the benchlens API, so benchlens' token and `read_public = false` do not
apply to it: anyone who can use Grafana (and with `GRAFANA_ANONYMOUS=true`, anyone who reaches the port) can
see every run and send any read-only SQL. Do not expose Grafana for a private benchlens. The port is
published on 127.0.0.1 only; **on a server**, set a real password, leave anonymous access off unless the
results are for everyone who can reach the port, then publish it with `GRAFANA_BIND=0.0.0.0`, preferably
behind a TLS proxy.
Grafana keeps reading while benchlens is stopped (benchlens keeps the SQLite `-wal` and `-shm` files that a
read-only reader cannot create). Versions, the dashboards' queries and the WAL details:
[deploy/grafana/README.md](../deploy/grafana/README.md).

## Upgrading

**The binary.** Replace it (or pull the new image) and start it on the same database. The database layout
has a version (`PRAGMA user_version`, shown as `db_layout` by `/api/health`). A newer benchlens migrates an
older layout automatically when it opens the file, in one transaction; tables and data are kept (layout 2
only recreated the views). An older benchlens refuses a newer layout instead of misreading it:

```text
$ benchlens --db future.db runs
Error: database layout 3 is newer than this benchlens understands (2); upgrade benchlens
```

(`future.db` was a copy with its `user_version` set to 3 by hand.) Back up the database file before
upgrading if you may want to go back.

**A newer vmbench.** Each release of benchlens carries the metric tables of the vmbench versions it knows:

```text
$ benchlens catalog list
0.2.1        98 metrics  built in
0.2.2        98 metrics  built in
0.2.3        98 metrics  built in
```

Runs of a vmbench version without a table are imported, flagged `[no metric table]`, and their values are
not judged. Add the table from the new vmbench itself; it is stored in the database, and runs already
imported are judged from then on:

```text
$ vmbench run --metrics > metrics.txt          # on a machine with the new vmbench
$ benchlens catalog add 0.3.0 metrics.txt
added 98 metrics for vmbench 0.3.0
$ benchlens catalog list
0.2.1        98 metrics  built in
0.2.2        98 metrics  built in
0.2.3        98 metrics  built in
0.3.0        98 metrics  added
```

(For this example `metrics.txt` was a copy of the built-in 0.2.3 table, `crates/rules/catalog/vmbench-0.2.3.txt`,
since no vmbench 0.3.0 exists yet; the `vmbench` line was not run.) A file without
`id|unit|direction|tool|label|description` lines is refused. If vmbench changes its result format it raises
`schema` in `meta.json`, and benchlens refuses those folders with a clear message until a release learns the
new schema ([contract.md](contract.md#when-vmbench-changes-its-format)).

## Reading the database from other tools

The SQLite file has three views for other tools (SQLite has no median, so verdicts come only from benchlens):

| View | Columns |
|---|---|
| `v_runs` | `run_id`, `folder`, `label`, `arch`, `nproc`, `ram_gb`, `cpu_model`, `vmbench_version`, `pg_profile`, `web_cpus`, `stages`, `start_utc`, `end_utc`, `auto_group`, `source`, `imported_at`, `meta_json`, `hardware_json`, `grp` (the effective group), `catalog_known` |
| `v_values` | every value that counts (failed and excluded stages left out): `run_id`, `label`, `grp`, `arch`, `nproc`, `ram_gb`, `cpu_model`, `vmbench_version`, `id`, `name`, `section`, `unit`, `direction`, `tool`, `round`, `value` |
| `v_stats` | per run and metric: `run_id`, `label`, `grp`, `id`, `name`, `section`, `unit`, `direction`, `n`, `mean`, `min`, `max` |

Open the file read-only from other tools while benchlens is running.
