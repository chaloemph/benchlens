# Wave 1 review: F (release workflow, Dockerfile, compose, install docs)

Branch `m2/f-packaging` at f654451. Verdict: **ok, fixed in review** (integration with C1).

## What was checked

- Dockerfile: Alpine build stage (static musl binary), `gcr.io/distroless/static-debian12:nonroot`, user
  65532, `/data` owned by it, `BENCHLENS_DB` / `BENCHLENS_LISTEN` set. Built locally from the final
  integration tree: 7.61 MB; `docker run -p 127.0.0.1:18081:8080 -v VOLUME:/data` answered `/api/health`,
  a second container imported the fixtures into the same volume, `/api/compare` and the UI answered 200.
  Container, volume and image removed afterwards.
- release.yml: tag must equal the workspace version; five targets; musl via cargo-zigbuild (needed for the
  bundled SQLite C code); Windows with static CRT; smoke test on every target that can run on its runner;
  `SHA256SUMS`; `gh release create --verify-tag`; ghcr push only on tags (metadata-action adds `latest` on
  semver tags, which compose pulls). `workflow_dispatch` gives a dry run without release or push.
- `[profile.release]` (thin LTO, one codegen unit, strip): does not change E's wasm bytes.
- README install section matches the workflow's archive names.

## Fixed in review

- `review(F)`: `serve --inbox` now also reads `BENCHLENS_INBOX` (as `watch` does), and the compose file shows
  how to turn the inbox on with it and a bind mount. `.dockerignore` also skips nested `target/` folders
  (`web/dev/server/target`) and `node_modules/`.

## Not verified (CI cannot either)

- release.yml has never run. A `workflow_dispatch` dry run on GitHub is needed before tagging v0.1.0
  (follow-up in the plan); the multi-arch docker build under QEMU and the macOS/Windows builds are untested.
- `docker compose up` itself was not run (the same image and environment were).
