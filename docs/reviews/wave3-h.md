# Wave 3 review: H (user guide, walkthrough, CONTRIBUTING, SECURITY, CHANGELOG, README)

Branch `m3/h-docs` at 9956a40, reviewed against docs/plan.md (Wave 3, H). Verdict: **ok, completed in review**
(the in-flight notes replaced once S, U and G were merged).

## What was checked

- README as a front page: quick start on the example data, documentation table, install from a release
  (asset names `benchlens-$VERSION-$TARGET.tar.gz` / `.zip` and `SHA256SUMS` match release.yml's Package
  and checksums steps), from source (Rust 1.89, the workspace `rust-version`), Docker, Compose.
- guide.md: commands and outputs are real (spot-checked the import, tag, inbox and server sections against
  the integrated build). walkthrough.md: clone to comparison with screenshots of the real UI.
- SECURITY.md: private vulnerability reporting through GitHub, supported versions, the security model.
  Checked every claim against the code: archive limits (512 MiB, 200,000 entries, 32 MiB per kept file),
  unsafe entries, kept files list, upload streaming and token-before-body, the write rules, token rules
  (16 printable ASCII, constant time, no flag, redacted), query string checks, `textContent` only, export
  CSP, CSV formula prefix. Accurate.
- CONTRIBUTING: setup, layout, the "adding a vmbench version" steps (catalog + fixture + golden), tests.
- CHANGELOG: v0.1.0 "unreleased", Keep-a-Changelog style.

## Completed in review (`review(H)` on the integration branch)

- Every `TODO(H, in flight)` note is gone: guide (registry for uploads with the new name, Host check and
  `allowed_hosts`, proxy headers and what they do not catch, split-by-tag, report.md dialog, `kept_files`,
  a Grafana section with the server checklist, SIGPIPE / 141), SECURITY.md (Host check and proxy headers
  under "protects"; remaining gaps: proxies that add no header, rebinding reads with a token and public
  reads, Grafana defaults and Grafana bypassing `read_public = false`; report.md gap closed), README
  (Grafana from G, SIGPIPE note from S with the pipefail remark). Each changed command was re-run on the
  integrated build and its output pasted (upload with token + registry, PATCH with `kept_files`, the four
  start-up variants with the new "Host names served" line, the toml + inbox server, Host refusals,
  `allowed_hosts`, proxy-header refusal, SIGPIPE status 141).
- CHANGELOG v0.1.0 lists the Wave 3 items (Host check, proxy headers, registry for uploads with the
  `inbox_registry` alias, `kept_files`, report.md dialog, split by tag, Grafana, SIGPIPE, kept WAL files).
- Link check (relative links and anchors in README, CHANGELOG, CONTRIBUTING, SECURITY, docs/*.md,
  deploy/grafana/README.md, tests/e2e/README.md): 68 links, 0 broken.

## Notes

- The Docker sections of the guide show output from an image built by H; the start-up lines now have the
  "Host names served" line, updated from the compose run of the integrated image.
