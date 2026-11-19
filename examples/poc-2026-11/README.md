# POC, November 2026

Registry for the 18 runs of a POC on six Proxmox VMs (x86 and ARM64 at 2, 8 and 16 vCPU), test round 1
(vmbench v0.2.1, with web rerun on v0.2.3) and round 2 (v0.2.3). The result folders themselves are not in
this repo; seven of them are in `tests/fixtures/`.

The round-1 full runs carry `exclude=web`: their web stage pinned the load generator to CPUs a small VM does
not have, and the `*-web-*` reruns replace it.

```bash
benchlens import --registry examples/poc-2026-11/registry.tsv path/to/round1 path/to/round2
benchlens compare group:x86-high group:arm64-high
```
