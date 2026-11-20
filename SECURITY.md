# Security

## Supported versions

| Version | Supported |
|---|---|
| 0.1.x (latest release) | yes |
| anything older, unreleased branches | no |

Fixes go into the next 0.1.x release; there are no backports to older releases.

## Reporting a vulnerability

Please do not open a public issue for a security problem.

Report it privately through GitHub: on [github.com/chaloemph/benchlens](https://github.com/chaloemph/benchlens),
open the **Security** tab and choose **Report a vulnerability** (GitHub private vulnerability reporting). The
report becomes a draft security advisory that only you and the maintainers can see. Include:

- the benchlens version (`benchlens --version`) and how it runs (binary, Docker, behind a proxy);
- what an attacker needs (network access, a token, a crafted archive, a page the user visits ...);
- steps or a file that reproduce it.

We will answer in the advisory, agree on a fix and a release, and credit you in the advisory unless you
prefer not to be named.

## Security model

benchlens is one binary with one SQLite file. It is built for a team that shares benchmark results, not for
hostile multi-tenant hosting: there is **one write token**, no users and no roles.

### What it protects

- **Result archives and folders are untrusted.** They come from the command line, the inbox, and uploads.
  Archives (`.tar`, `.tar.gz`) are read in memory and never extracted to disk. An entry with an absolute path,
  a `..` component, a link, a device or any other special type refuses the whole archive. An archive is
  refused after 512 MiB of data or 200,000 entries; one kept file may be at most 32 MiB. Only the files of the
  data contract are kept (`meta.json`, `results.tsv`, `status.tsv`, `hardware.json`, `report.md`,
  `benchlens-tags.json`); everything else is ignored. Folders: links are not followed. A `meta.json` with an
  unknown `schema` is refused instead of being read wrongly ([docs/contract.md](docs/contract.md)).
- **Uploads** (`POST /api/runs`) are streamed to a temporary folder and counted as they arrive: one byte over
  `max_upload_mb` (default 256 MiB) ends the request with 413. The token check runs before the body is read,
  so a refused upload is not received. The temporary folder is removed on every path.
- **Writes** (upload, tag editing, delete):
  - with a token configured, they need `Authorization: Bearer <token>`, else 401;
  - without a token, they are open only when the server listens on a loopback address (`127.0.0.1`,
    `::1`); on any other address they are refused (403) and the server warns at start;
  - without a token, a write that a browser marks as coming from a page of another site
    (`Sec-Fetch-Site: cross-site`, or an `Origin` that does not name the `Host`) is refused (403), so a web
    page the local user visits cannot upload to a local server;
  - without a token, a write that carries `Forwarded`, `X-Forwarded-For` or `X-Forwarded-Host` is refused
    (403): it came through a reverse proxy, where "local" no longer means local.
- **DNS rebinding**: without a token, a request is served only when its `Host` (and the authority of the
  request target, if any) is `localhost`, an IP address or a name in `allowed_hosts`, any port; anything else
  is 403, for the web UI files and the API alike. A page whose domain an attacker re-points at 127.0.0.1
  sends that domain as `Host` and is refused; an IP address cannot be re-pointed. With a token, any `Host`
  is served unless `allowed_hosts` is set.
- **Reads**: open by default. With `read_public = false`, every `/api` call except `/api/health` needs the
  token, including the export endpoint and kept files (the web UI fetches `report.md` with the token). The
  server refuses to start with `read_public = false` and no token.
- **The token** must be at least 16 printable ASCII characters (`openssl rand -hex 32`). It has no
  command-line flag (command lines are visible to other users of the machine): set `BENCHLENS_TOKEN` or
  `token` in `benchlens.toml`. It is compared in constant time (SHA-256 of both sides), never printed and
  redacted from debug output. The web UI keeps it in `sessionStorage` for the tab only.
- **Input checks**: query strings are checked before anything is looked up (unknown or repeated keys, values
  over 1024 bytes or with control characters are 400); tag keys are 1-64 of `A-Z a-z 0-9 _ - .`, values one
  line of at most 1024 bytes. SQL is always parameterised.
- **The web UI and the HTML export** put data into the page as text only (never `innerHTML`). The exported
  HTML carries a Content-Security-Policy that allows no network access, and its inlined JSON escapes `<`,
  `>` and `&`. CSV export prefixes text starting with `= + - @` with `'` so spreadsheets do not run it as a
  formula.

### What it does not protect

- **No TLS.** benchlens speaks plain HTTP. On anything but a trusted network, put a reverse proxy with TLS in
  front (Caddy, nginx), or the token travels in the clear.
- **A reverse proxy on the same host** makes a loopback listener reachable from outside, and loopback without
  a token means "writes open". The forwarded-header check above catches proxies that add those headers, not
  one that adds none (nginx's plain `proxy_pass`). Always set a token when anything but the local machine
  can reach the server.
- **DNS rebinding with a token and public reads**: with a token, any `Host` is served (the token itself is
  safe: a rebinding page never sees it), so a rebinding page can read what `read_public = true` makes
  public, also from a server on 127.0.0.1. Set `allowed_hosts` (or `read_public = false`) if the data of a
  local server must not reach web pages the user visits.
- **Anyone with the token can do everything**: upload, retag, delete. There is no audit log beyond each
  run's `source` (`cli`, `inbox`, `upload`) and import time.
- **Reads are public by default.** Benchmark results and `hardware.json` (CPU model, memory, disks; vmbench
  writes no serials, host names or addresses) are visible to anyone who can reach the server unless
  `read_public = false`.
- **On a private server** (`read_public = false`) the report.md link opened in a new tab (middle click,
  "open in new tab") is a plain request without the token and gets 401: it fails closed, it does not leak.
  A plain click shows the file in the page, fetched with the token.
- **Grafana** (compose profile `grafana`) reads the database read-only, but its defaults are for one
  machine: admin password `admin` unless `GRAFANA_ADMIN_PASSWORD` is set, anonymous viewers, port 3000 on
  127.0.0.1. Before publishing the port (`GRAFANA_BIND`), set a real password and decide on anonymous
  access ([deploy/grafana/README.md](deploy/grafana/README.md)). Grafana has its own users, separate from
  the benchlens token, and it can show every run, also on a benchlens server with `read_public = false`.
- **The database file** is not encrypted; protect it with file permissions. Anyone who can write it can
  change any result.
- **The inbox folder** imports whatever is placed in it: whoever can write there can add runs (archives are
  still checked as above). Its lock file (`.benchlens.lock`) holds the watcher's pid, host name and start
  time.
- **Denial of service** beyond the size limits (many concurrent uploads, very large databases) is not
  defended against; put rate limits in the proxy if the server is exposed.
