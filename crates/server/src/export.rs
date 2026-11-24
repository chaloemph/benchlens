//! The static HTML export: one self-contained file with a comparison that opens from disk (file://), with no
//! server and no network. Used by `benchlens export html` and `GET /api/export.html`.
//!
//! The file is the live UI itself, not a copy that could drift: `web/index.html` with `web/css/app.css` and
//! d3 inlined, every ES module reachable from `web/export/offline.js` (which reuses `web/js/main.js` and the
//! views) bundled into one classic script by [`bundle`], the WASM build of the verdict rule as base64, and the
//! exported sides with their values ([`ExportData`]) as JSON. In the page, `offline.js` answers the UI's API
//! calls from that JSON and computes every comparison with the WASM `compare`, so the numbers equal
//! `benchlens compare --json` for the same sides.
//!
//! Escaping: the data goes into a `<script type="application/json">` with `<`, `>`, `&`, U+2028 and U+2029
//! written as `\uXXXX` ([`json_for_script`]), so no label or tag can close the element or open a comment; the
//! title is HTML-escaped; the page renders every value with `textContent`. Inlined code and CSS are refused
//! when they contain `</script`, `</style` or `<!--` ([`check_inline`]). The page carries a
//! Content-Security-Policy ([`CSP`]) without network access.

use crate::params::{all, bad_request, check_query, check_ref, db, from_query_rejection, required};
use crate::{ApiError, AppState, Web};
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use benchlens_core::compare::{self, Reference, Side, ValueRow};
use benchlens_core::store::{GroupSummary, RunDetail, Store};
use serde::Serialize;
use std::collections::BTreeSet;
use std::sync::OnceLock;

/// Version of the JSON inlined in an export (`format` field); raise it when the shape changes.
pub const FORMAT: u32 = 1;

/// Content-Security-Policy of an exported page (meta tag) and of `GET /api/export.html` (header): inline
/// script and style only, WebAssembly compiled from the inlined bytes, nothing loaded from anywhere.
pub const CSP: &str = "default-src 'none'; script-src 'unsafe-inline' 'wasm-unsafe-eval'; \
style-src 'unsafe-inline'; img-src data:; base-uri 'none'; form-action 'none'";

/// The module the export page starts from (path inside `web/`).
const ENTRY: &str = "export/offline.js";

/// One side of the export: what `core::compare::load` gives for its reference.
#[derive(Debug, Clone, Serialize)]
pub struct ExportSide {
    pub side: Side,
    pub values: Vec<ValueRow>,
}

/// Everything inlined in an exported page (docs/api.md, `GET /api/export.html`).
#[derive(Debug, Clone, Serialize)]
pub struct ExportData {
    /// [`FORMAT`].
    pub format: u32,
    /// benchlens version that made the file.
    pub benchlens: String,
    /// When, `YYYY-MM-DDTHH:MM:SSZ`.
    pub exported_at: String,
    /// Where the data came from (database file name, or the server).
    pub source: String,
    pub title: String,
    /// The exported selection: baseline reference and compared references, as given.
    pub base: String,
    pub others: Vec<String>,
    /// Baseline first, then each compared side, each with every value that counts for it.
    pub sides: Vec<ExportSide>,
    /// Every run of those sides (summary, meta.json, hardware.json; `files` left empty).
    pub runs: Vec<RunDetail>,
    /// Summaries of the groups among the sides (`group:NAME`).
    pub groups: Vec<GroupSummary>,
}

/// Load the sides of a comparison and everything the export page shows about them.
pub fn collect(
    store: &Store,
    base: &str,
    others: &[String],
    source: &str,
    title: Option<&str>,
) -> benchlens_core::Result<ExportData> {
    if others.is_empty() {
        return Err(benchlens_core::Error::NotFound(
            "nothing to compare the baseline with".into(),
        ));
    }
    let mut sides = Vec::new();
    for reference in std::iter::once(base).chain(others.iter().map(String::as_str)) {
        let (side, values) = compare::load(store, reference)?;
        sides.push(ExportSide { side, values });
    }
    let mut run_ids: Vec<&String> = Vec::new();
    for s in &sides {
        for id in &s.side.run_ids {
            if !run_ids.contains(&id) {
                run_ids.push(id);
            }
        }
    }
    let runs = run_ids
        .into_iter()
        .map(|id| {
            let mut d = store.run_detail(id)?;
            d.files.clear();
            Ok(d)
        })
        .collect::<benchlens_core::Result<Vec<_>>>()?;
    // group:NAME and group:NAME@KEY=VALUE both name the group NAME
    let group_names: BTreeSet<&str> = sides
        .iter()
        .filter_map(|s| match Reference::parse(&s.side.reference) {
            Ok(Reference::Group { name, .. }) => Some(name),
            _ => None,
        })
        .collect();
    let groups = store
        .list_groups()?
        .into_iter()
        .filter(|g| group_names.contains(g.group.as_str()))
        .collect();
    let title = match title.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => t.to_string(),
        None => format!(
            "{} vs {}",
            sides[0].side.label,
            sides[1..]
                .iter()
                .map(|s| s.side.label.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    Ok(ExportData {
        format: FORMAT,
        benchlens: env!("CARGO_PKG_VERSION").into(),
        exported_at: utc_now(),
        source: source.into(),
        title,
        base: base.into(),
        others: others.to_vec(),
        sides,
        runs,
        groups,
    })
}

/// The exported page for `data`.
pub fn render_html(data: &ExportData) -> anyhow::Result<String> {
    let assets = static_assets()
        .as_ref()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let json = json_for_script(&serde_json::to_string(data)?);
    let mut out = String::with_capacity(
        assets.head.len() + assets.tail.len() + json.len() + data.title.len() + 256,
    );
    out.push_str(&assets.head);
    out.push_str("<title>");
    out.push_str(&escape_html(&data.title));
    out.push_str(" · benchlens</title>");
    out.push_str(&assets.middle);
    out.push_str("<script type=\"application/json\" id=\"benchlens-data\">");
    out.push_str(&json);
    out.push_str("</script>\n");
    out.push_str(&assets.tail);
    Ok(out)
}

/// The parts of the page that do not depend on the data, built once.
struct Assets {
    /// From `<!doctype html>` up to where `<title>` goes.
    head: String,
    /// From after `<title>` up to where the data goes (inside `<body>`, after the markup).
    middle: String,
    /// The WASM, the bundled code and `</body></html>`.
    tail: String,
}

fn static_assets() -> &'static Result<Assets, String> {
    static ASSETS: OnceLock<Result<Assets, String>> = OnceLock::new();
    ASSETS.get_or_init(|| build_assets().map_err(|e| format!("building the export page: {e:#}")))
}

fn web_text(path: &str) -> anyhow::Result<String> {
    let file = Web::get(path)
        .ok_or_else(|| anyhow::anyhow!("web/{path} is not built into this binary"))?;
    Ok(String::from_utf8(file.data.into_owned())?)
}

/// `<script src>` / `<link href>` value of a line of index.html.
fn attr<'a>(line: &'a str, name: &str) -> anyhow::Result<&'a str> {
    let key = format!("{name}=\"");
    let start = line
        .find(&key)
        .ok_or_else(|| anyhow::anyhow!("no {name}= in {line:?}"))?
        + key.len();
    let len = line[start..]
        .find('"')
        .ok_or_else(|| anyhow::anyhow!("unclosed {name}= in {line:?}"))?;
    Ok(&line[start..start + len])
}

fn build_assets() -> anyhow::Result<Assets> {
    let index = web_text("index.html")?;
    let mut head = String::new();
    let mut middle = String::new();
    let mut in_head = true;
    // title, module entry, </body>
    let mut saw = (false, false, false);
    // export/export.css is inlined once, after the last stylesheet of index.html
    let last_css = index
        .lines()
        .enumerate()
        .filter(|(_, l)| l.trim().starts_with("<link rel=\"stylesheet\""))
        .map(|(n, _)| n)
        .last()
        .ok_or_else(|| {
            anyhow::anyhow!("web/index.html has no stylesheet to append export/export.css to")
        })?;
    for (n, line) in index.lines().enumerate() {
        let t = line.trim();
        let out = if in_head { &mut head } else { &mut middle };
        if t.starts_with("<title>") {
            saw.0 = true;
            in_head = false;
            continue;
        } else if t == "<meta charset=\"utf-8\">" {
            out.push_str(line);
            out.push_str("\n<meta http-equiv=\"Content-Security-Policy\" content=\"");
            out.push_str(CSP);
            out.push_str("\">\n<meta name=\"generator\" content=\"benchlens export\">\n");
            continue;
        } else if t.starts_with("<link rel=\"stylesheet\"") {
            let href = attr(t, "href")?;
            let mut css = web_text(href)?;
            if n == last_css {
                css.push('\n');
                css.push_str(&web_text("export/export.css")?);
            }
            check_inline(href, &css)?;
            out.push_str("<style>\n");
            out.push_str(&css);
            out.push_str("\n</style>\n");
            continue;
        } else if t.starts_with("<script type=\"module\"") {
            saw.1 = true;
            continue;
        } else if t.starts_with("<script") && t.contains(" src=") {
            let src = attr(t, "src")?;
            let js = web_text(src)?;
            check_inline(src, &js)?;
            out.push_str("<script>");
            out.push_str(&js);
            out.push_str("\n</script>\n");
            continue;
        } else if t == "</body>" {
            saw.2 = true;
            break;
        }
        if t.contains(" src=\"") || t.starts_with("<link") {
            anyhow::bail!("web/index.html: cannot inline {t:?}");
        }
        out.push_str(line);
        out.push('\n');
    }
    if saw != (true, true, true) {
        anyhow::bail!(
            "web/index.html must have a <title> line, a <script type=\"module\"> entry and a </body> line"
        );
    }
    let wasm = Web::get("wasm/benchlens_wasm_bg.wasm").ok_or_else(|| {
        anyhow::anyhow!("web/wasm/benchlens_wasm_bg.wasm is not built into this binary")
    })?;
    let code = bundle(ENTRY)?;
    check_inline("bundle", &code)?;
    let mut tail = String::new();
    tail.push_str("<script type=\"application/octet-stream\" id=\"benchlens-wasm\">");
    tail.push_str(&base64(&wasm.data));
    tail.push_str("</script>\n<script>\n");
    tail.push_str(&code);
    tail.push_str("</script>\n</body>\n</html>\n");
    Ok(Assets { head, middle, tail })
}

/// Refuse code or CSS that would end its element early or start an HTML comment inside it.
pub fn check_inline(what: &str, text: &str) -> anyhow::Result<()> {
    let lower = text.to_ascii_lowercase();
    for bad in ["</script", "</style", "<!--"] {
        if lower.contains(bad) {
            anyhow::bail!("{what} contains {bad:?}; it cannot be inlined in the export");
        }
    }
    Ok(())
}

/// JSON (as serde_json writes it) made safe inside `<script type="application/json">`: `<`, `>` and `&` only
/// occur inside strings there, where `<` etc. mean the same; U+2028/2029 are escaped for old parsers.
pub fn json_for_script(json: &str) -> String {
    let mut out = String::with_capacity(json.len() + 64);
    for c in json.chars() {
        match c {
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c => out.push(c),
        }
    }
    out
}

/// Text for HTML element content or a quoted attribute.
pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// Standard base64 with padding.
pub fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16
            | (*chunk.get(1).unwrap_or(&0) as u32) << 8
            | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Now in UTC, `YYYY-MM-DDTHH:MM:SSZ`.
pub fn utc_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    utc_string(secs)
}

/// Seconds since the epoch as `YYYY-MM-DDTHH:MM:SSZ` (civil-from-days, proleptic Gregorian).
pub fn utc_string(secs: i64) -> String {
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

// ---- bundling the ES modules of web/ into one classic script ----

/// Bundle the ES module `entry` (a path inside `web/`) and every module it imports into one classic script.
///
/// Each module becomes a function whose result is its exports, evaluated after its imports (the order ES
/// modules evaluate in). This is not a general bundler: it accepts the plain style web/ is written in and
/// refuses anything else, so a module that would be bundled wrongly fails the export (and the tests) instead:
/// imports on one line (`import { a, b as c } from './x.js';`, `import x from`, `import x, { a } from`,
/// `import * as x from`), and exports as `export function` / `export async function` / `export const` /
/// `export class` / `export { a, b as c };` at the start of a line. `export let` / `export var` (live
/// bindings) and `export default` are refused. `import.meta.url` becomes `document.baseURI`.
pub fn bundle(entry: &str) -> anyhow::Result<String> {
    let mut b = Bundler::default();
    b.visit(entry, &mut Vec::new())?;
    let mut out = String::from(
        "(function () {\n\"use strict\";\nconst __benchlens_modules = Object.create(null);\n",
    );
    for (id, body) in &b.done {
        out.push_str(&format!(
            "__benchlens_modules[{}] = (function () {{\n{body}}})();\n",
            serde_json::to_string(id)?
        ));
    }
    out.push_str("})();\n");
    Ok(out)
}

#[derive(Default)]
struct Bundler {
    /// Modules in evaluation order, with their transformed bodies.
    done: Vec<(String, String)>,
}

impl Bundler {
    fn visit(&mut self, id: &str, stack: &mut Vec<String>) -> anyhow::Result<()> {
        if self.done.iter().any(|(d, _)| d == id) {
            return Ok(());
        }
        if stack.iter().any(|s| s == id) {
            anyhow::bail!("import cycle: {} -> {id}", stack.join(" -> "));
        }
        stack.push(id.to_string());
        let src = web_text(id)?;
        let (body, deps) = transform(id, &src)?;
        for d in &deps {
            self.visit(d, stack)?;
        }
        stack.pop();
        self.done.push((id.to_string(), body));
        Ok(())
    }
}

/// `spec` (relative, `./` or `../`) imported from module `from`, as a path inside `web/`.
fn resolve(from: &str, spec: &str) -> anyhow::Result<String> {
    if !(spec.starts_with("./") || spec.starts_with("../")) || !spec.ends_with(".js") {
        anyhow::bail!("{from}: only relative imports of .js files can be bundled, not {spec:?}");
    }
    let mut parts: Vec<&str> = from.split('/').collect();
    parts.pop();
    for seg in spec.split('/') {
        match seg {
            "." | "" => {}
            ".." => {
                if parts.pop().is_none() {
                    anyhow::bail!("{from}: {spec:?} points outside web/");
                }
            }
            s => parts.push(s),
        }
    }
    Ok(parts.join("/"))
}

fn is_ident(s: &str) -> bool {
    let mut c = s.chars();
    c.next()
        .is_some_and(|f| f.is_ascii_alphabetic() || f == '_' || f == '$')
        && c.all(|x| x.is_ascii_alphanumeric() || x == '_' || x == '$')
}

fn ident_at(s: &str) -> &str {
    let end = s
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$'))
        .unwrap_or(s.len());
    &s[..end]
}

/// `a, b as c` (inside braces) as `(imported_or_local, alias)` pairs.
fn name_list(what: &str, list: &str) -> anyhow::Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for item in list.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let (a, b) = match item.split_once(" as ") {
            Some((a, b)) => (a.trim(), b.trim()),
            None => (item, item),
        };
        if !is_ident(a) || !is_ident(b) {
            anyhow::bail!("{what}: cannot bundle {item:?}");
        }
        out.push((a.to_string(), b.to_string()));
    }
    Ok(out)
}

/// One module's body as a function body returning its exports, and the modules it imports.
fn transform(id: &str, src: &str) -> anyhow::Result<(String, Vec<String>)> {
    let mut body = String::with_capacity(src.len() + 256);
    let mut deps = Vec::new();
    let mut exports: Vec<(String, String)> = Vec::new(); // (exported, local)
    for (n, line) in src.lines().enumerate() {
        let at = || format!("web/{id}:{}", n + 1);
        let line = line.replace("import.meta.url", "document.baseURI");
        if let Some(rest) = line.strip_prefix("import ") {
            let rest = rest.trim().trim_end_matches(';').trim();
            let (clause, spec) = match rest.rsplit_once(" from ") {
                Some((c, s)) => (c.trim(), s.trim()),
                None => ("", rest),
            };
            let spec = spec
                .strip_prefix(['\'', '"'])
                .and_then(|s| s.strip_suffix(['\'', '"']))
                .ok_or_else(|| anyhow::anyhow!("{}: cannot bundle this import", at()))?;
            let dep = resolve(id, spec)?;
            let m = format!("__benchlens_modules[{}]", serde_json::to_string(&dep)?);
            deps.push(dep);
            let (default, named) = match clause.find('{') {
                Some(i) => {
                    let close = clause
                        .rfind('}')
                        .filter(|&j| j > i)
                        .ok_or_else(|| anyhow::anyhow!("{}: imports must be on one line", at()))?;
                    (
                        clause[..i].trim().trim_end_matches(',').trim(),
                        Some(&clause[i + 1..close]),
                    )
                }
                None => (clause, None),
            };
            if let Some(ns) = default.strip_prefix("* as ") {
                if !is_ident(ns.trim()) {
                    anyhow::bail!("{}: cannot bundle this import", at());
                }
                body.push_str(&format!("const {} = {m};\n", ns.trim()));
            } else if !default.is_empty() {
                if !is_ident(default) {
                    anyhow::bail!("{}: cannot bundle this import", at());
                }
                body.push_str(&format!("const {default} = {m}.default;\n"));
            }
            if let Some(list) = named {
                let pairs = name_list(&at(), list)?;
                let fields: Vec<String> = pairs
                    .iter()
                    .map(|(a, b)| {
                        if a == b {
                            a.clone()
                        } else {
                            format!("{a}: {b}")
                        }
                    })
                    .collect();
                body.push_str(&format!("const {{ {} }} = {m};\n", fields.join(", ")));
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("export ") {
            let rest = rest.trim_start();
            if let Some(list) = rest.strip_prefix('{') {
                let list = list
                    .trim_end()
                    .trim_end_matches(';')
                    .trim_end()
                    .strip_suffix('}')
                    .ok_or_else(|| anyhow::anyhow!("{}: export lists must be on one line", at()))?;
                for (local, exported) in name_list(&at(), list)? {
                    exports.push((exported, local));
                }
                continue;
            }
            let decl = rest
                .strip_prefix("async function")
                .or_else(|| rest.strip_prefix("function"))
                .map(|r| r.trim_start_matches('*'))
                .or_else(|| rest.strip_prefix("const "))
                .or_else(|| rest.strip_prefix("class "));
            let name = decl.map(|d| ident_at(d.trim_start())).unwrap_or("");
            if !is_ident(name) {
                anyhow::bail!(
                    "{}: cannot bundle this export (use export function / const / class / {{ list }})",
                    at()
                );
            }
            exports.push((name.to_string(), name.to_string()));
            body.push_str(rest);
            body.push('\n');
            continue;
        }
        let t = line.trim_start();
        if (t.starts_with("import ") || t.starts_with("export ")) && t.len() != line.len() {
            anyhow::bail!(
                "{}: import / export must start at the beginning of a line",
                at()
            );
        }
        body.push_str(&line);
        body.push('\n');
    }
    let fields: Vec<String> = exports
        .iter()
        .map(|(e, l)| {
            if e == l {
                e.clone()
            } else {
                format!("{e}: {l}")
            }
        })
        .collect();
    body.push_str(&format!("return {{ {} }};\n", fields.join(", ")));
    Ok((body, deps))
}

// ---- GET /api/export.html ----

/// File name offered for a download: `benchlens-<base>-vs-<other>.html`, letters, digits, `. _ -` only.
pub fn file_name(base: &str, others: &[String]) -> String {
    let part = |r: &str| {
        let s: String = r
            .trim_start_matches("group:")
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                    c
                } else {
                    '-'
                }
            })
            .take(40)
            .collect();
        let s = s.trim_matches('-').to_string();
        if s.is_empty() {
            "side".to_string()
        } else {
            s
        }
    };
    let mut names = vec![part(base)];
    names.extend(others.iter().take(3).map(|o| part(o)));
    format!("benchlens-{}.html", names.join("-vs-"))
}

/// `GET /api/export.html?base=REF&other=REF[&other=REF...][&title=T]`: the static HTML report, as a download.
pub async fn export_html(
    State(st): State<AppState>,
    q: Result<Query<Vec<(String, String)>>, QueryRejection>,
) -> Result<Response, ApiError> {
    let Query(p) = q.map_err(from_query_rejection)?;
    check_query(&p, &["base", "other", "title"])?;
    let base = required(&p, "base")?.to_string();
    check_ref("base", &base)?;
    let others: Vec<String> = all(&p, "other").into_iter().map(str::to_string).collect();
    if others.is_empty() {
        return Err(bad_request(
            "nothing to compare the baseline with: give at least one other=REF",
        ));
    }
    for o in &others {
        check_ref("other", o)?;
    }
    let title = crate::params::single(&p, "title")?.map(str::to_string);
    let name = file_name(&base, &others);
    let data = db(&st, move |s| {
        Ok(collect(
            s,
            &base,
            &others,
            "benchlens server",
            title.as_deref(),
        )?)
    })
    .await?;
    let html = render_html(&data).map_err(|e| ApiError::internal(format!("{e:#}")))?;
    Ok((
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}\""),
            ),
            (header::CONTENT_SECURITY_POLICY, CSP.to_string()),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
        ],
        html,
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648() {
        for (raw, enc) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(raw.as_bytes()), enc);
        }
        assert_eq!(base64(&[0xff, 0xfe, 0x00]), "//4A");
    }

    #[test]
    fn utc_dates() {
        assert_eq!(utc_string(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc_string(1_795_093_509), "2026-11-19T13:05:09Z");
        assert_eq!(utc_string(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(utc_now().len(), 20);
    }

    #[test]
    fn json_cannot_close_the_script() {
        let v = serde_json::json!({"label": "</script><img src=x onerror=alert(1)><!-- \u{2028}\u{2029} & </SCRIPT"});
        let s = json_for_script(&serde_json::to_string(&v).unwrap());
        assert!(!s.contains('<') && !s.contains('>') && !s.contains('&'));
        assert!(!s.contains('\u{2028}') && !s.contains('\u{2029}'));
        let back: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn html_escape() {
        assert_eq!(
            escape_html("<b a=\"x\">'&'</b>"),
            "&lt;b a=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/b&gt;"
        );
    }

    #[test]
    fn inline_check() {
        assert!(check_inline("x", "a = '</SCRIPT>'").is_err());
        assert!(check_inline("x", "x <!-- y").is_err());
        assert!(check_inline("x", "p { } </style>").is_err());
        assert!(check_inline("x", "if (a < b) c();").is_ok());
    }

    #[test]
    fn resolve_paths() {
        assert_eq!(
            resolve("export/offline.js", "../js/api.js").unwrap(),
            "js/api.js"
        );
        assert_eq!(resolve("js/main.js", "./data.js").unwrap(), "js/data.js");
        assert!(resolve("js/main.js", "../../x.js").is_err());
        assert!(resolve("js/main.js", "d3").is_err());
        assert!(resolve("js/main.js", "https://x/y.js").is_err());
    }

    #[test]
    fn transform_imports_and_exports() {
        let src = "import init, { compare as c, version } from '../wasm/w.js';\n\
                   import { h } from './ui.js';\n\
                   import * as f from './f.js';\n\
                   export const X = 1;\n\
                   export function g() { return h(c); }\n\
                   export async function k() {}\n\
                   export class Y {}\n\
                   const z = new URL('a', import.meta.url);\n\
                   export { z, g as default };\n";
        let (body, deps) = transform("js/m.js", src).unwrap();
        assert_eq!(deps, ["wasm/w.js", "js/ui.js", "js/f.js"]);
        assert!(body.contains("const init = __benchlens_modules[\"wasm/w.js\"].default;"));
        assert!(
            body.contains("const { compare: c, version } = __benchlens_modules[\"wasm/w.js\"];")
        );
        assert!(body.contains("const f = __benchlens_modules[\"js/f.js\"];"));
        assert!(body.contains("\nconst X = 1;\nfunction g()"));
        assert!(body.contains("document.baseURI"));
        assert!(body.ends_with("return { X, g, k, Y, z, default: g };\n"));
        assert!(!body.contains("export "));
        assert!(!body.contains("import "));
    }

    #[test]
    fn transform_refuses_what_it_cannot_bundle() {
        for src in [
            "export let x = 1;\n",
            "export default 3;\n",
            "export var y;\n",
            "import {\n  a } from './a.js';\n",
            "import x from 'd3';\n",
            "  export const a = 1;\n",
            "export {\n a };\n",
        ] {
            assert!(transform("js/m.js", src).is_err(), "{src:?}");
        }
    }

    #[test]
    fn web_ui_bundles() {
        let code = bundle(ENTRY).unwrap();
        for m in [
            "js/main.js",
            "js/compare.js",
            "js/detail.js",
            "wasm/benchlens_wasm.js",
            ENTRY,
        ] {
            assert!(
                code.contains(&format!("__benchlens_modules[\"{m}\"] =")),
                "{m}"
            );
        }
        // dependencies come before what imports them
        let pos = |m: &str| {
            code.find(&format!("__benchlens_modules[\"{m}\"] ="))
                .unwrap()
        };
        assert!(pos("js/api.js") < pos("js/data.js"));
        assert!(pos("js/main.js") < pos(ENTRY));
        assert!(pos("wasm/benchlens_wasm.js") < pos(ENTRY));
        // the live UI's own entry bundles too (every module of web/js is in the supported style)
        bundle("js/app.js").unwrap();
    }

    #[test]
    fn export_css_is_inlined_once_after_the_stylesheets() {
        let assets = static_assets().as_ref().unwrap();
        let page = format!("{}{}", assets.head, assets.middle);
        let export_css = web_text("export/export.css").unwrap();
        assert_eq!(page.matches(export_css.trim()).count(), 1);
        // the Summary view's stylesheet is in the file, and before export.css (which may override it)
        let summary = web_text("css/summary.css").unwrap();
        let at = page.find(summary.trim()).expect("css/summary.css");
        assert!(at < page.find(export_css.trim()).unwrap());
    }

    #[test]
    fn summary_modules_are_bundled() {
        let code = bundle(ENTRY).unwrap();
        for m in ["js/summary/index.js", "js/summary/board.js", "js/score.js"] {
            assert!(
                code.contains(&format!("__benchlens_modules[\"{m}\"] =")),
                "{m}"
            );
        }
    }

    #[test]
    fn file_names_are_plain() {
        assert_eq!(
            file_name("group:x86-high", &["group:arm64-high".into()]),
            "benchlens-x86-high-vs-arm64-high.html"
        );
        assert_eq!(
            file_name("../../\"evil\r\n", &["<x>".into()]),
            "benchlens-..-..--evil-vs-x.html"
        );
    }
}
