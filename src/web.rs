//! Loopback-only browser UI. Assets ship in the binary; no frontend build or CDN.
use crate::{Vault, board::Board, git, vault};
use anyhow::{Context, Result, bail};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Query, Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use pulldown_cmark::{Event, Options, Parser, Tag, html};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    process::Command,
    sync::Arc,
};

#[derive(Clone)]
struct App {
    vault: Vault,
    authority: String,
}

type Shared = Arc<App>;
type ApiResult = std::result::Result<Json<Value>, ApiError>;
struct ApiError(StatusCode, String);
impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        let message = format!("{error:#}");
        let status = if message.contains("conflict") {
            StatusCode::CONFLICT
        } else {
            StatusCode::BAD_REQUEST
        };
        Self(status, message)
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error": self.1}))).into_response()
    }
}

fn config_path() -> Result<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))
        .context("cannot locate user config directory")?;
    Ok(base.join("agent-context/web.json"))
}

/// Web-only fallback. Existing MCP and CLI root resolution remains unchanged.
pub fn open_vault(explicit: Option<&Path>) -> Result<Vault> {
    if let Some(path) = explicit {
        return Vault::open(path);
    }
    if let Ok(vault) = Vault::open(&std::env::current_dir()?) {
        return Ok(vault);
    }
    let config = config_path()?;
    if config.exists() {
        #[derive(Deserialize)]
        struct Config {
            root: PathBuf,
        }
        let saved: Config = serde_json::from_slice(&fs::read(&config)?)
            .with_context(|| format!("invalid config: {}", config.display()))?;
        return Vault::open(&saved.root);
    }
    bail!("首次启动请指定 vault：agent-context web --root <目录>；之后只需 agent-context web")
}

pub async fn serve(vault: Vault, port: u16, open_browser: bool) -> Result<()> {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .with_context(|| format!("port {port} unavailable; try agent-context web --port 0"))?;
    let authority = listener.local_addr()?.to_string();
    let url = format!("http://{authority}");
    let config = config_path()?;
    fs::create_dir_all(config.parent().unwrap())?;
    fs::write(
        &config,
        serde_json::to_vec_pretty(&json!({"root": vault.root()}))?,
    )?;
    println!(
        "Agent Context: {url}\nVault: {}\n按 Ctrl+C 停止服务。下次启动：agent-context web",
        vault.root().display()
    );
    if open_browser && let Err(error) = browse(&url) {
        eprintln!("无法自动打开浏览器：{error}。请访问 {url}");
    }
    axum::serve(listener, router(vault, authority))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
fn browse(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    let status = Command::new("open").arg(url).status()?;
    #[cfg(target_os = "windows")]
    let status = Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .status()?;
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let status = Command::new("xdg-open").arg(url).status()?;
    if !status.success() {
        bail!("browser opener exited with {status}");
    }
    Ok(())
}

pub fn router(vault: Vault, authority: String) -> Router {
    let state = Arc::new(App { vault, authority });
    Router::new()
        .route("/", get(|| async { Html(include_str!("web/index.html")) }))
        .route(
            "/app.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("web/app.js"),
                )
            }),
        )
        .route(
            "/app.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    include_str!("web/app.css"),
                )
            }),
        )
        .route("/api/catalog", get(catalog))
        .route("/api/document", get(document).put(save_document))
        .route("/api/board", post(save_board).delete(delete_board))
        .route("/api/preview", post(preview))
        .route("/api/git", get(git_preview).post(git_commit))
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state)
}
async fn guard(State(app): State<Shared>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    let host_ok = headers.get(header::HOST).and_then(|h| h.to_str().ok()) == Some(&app.authority);
    let origin = format!("http://{}", app.authority);
    let origin_ok = headers
        .get(header::ORIGIN)
        .is_none_or(|h| h.to_str().ok() == Some(&origin));
    let site_ok = headers
        .get("sec-fetch-site")
        .is_none_or(|h| h == "same-origin" || h == "none");
    let write_ok = matches!(*request.method(), Method::GET | Method::HEAD)
        || headers.get("x-agent-context").is_some_and(|h| h == "1");
    if !(host_ok && origin_ok && site_ok && write_ok) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"仅允许从本机页面访问"})),
        )
            .into_response();
    }
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    for (name, value) in [
        ("cache-control", "no-store"),
        ("x-content-type-options", "nosniff"),
        ("referrer-policy", "no-referrer"),
        (
            "content-security-policy",
            "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'",
        ),
    ] {
        headers.insert(
            header::HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    }
    response
}
async fn work<F>(app: Shared, function: F) -> ApiResult
where
    F: FnOnce(&Vault) -> Result<Value> + Send + 'static,
{
    tokio::task::spawn_blocking(move || function(&app.vault))
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .map_err(Into::into)
}

pub fn revision(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

#[derive(Serialize)]
struct Page {
    path: String,
    title: String,
    summary: String,
    tags: Vec<String>,
    revision: String,
    compiled: bool,
    search_text: String,
}
fn page(vault: &Vault, path: String, compiled: bool) -> Result<Page> {
    let text = vault.read(&path)?;
    let fm = vault::frontmatter(&text).unwrap_or_default();
    let body = vault::body(&text);
    let title = fm
        .get("title")
        .map(|s| vault::scalar(s))
        .or_else(|| {
            body.lines()
                .find_map(|s| s.strip_prefix("# ").map(String::from))
        })
        .unwrap_or_else(|| {
            Path::new(&path)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
    let summary = body
        .lines()
        .map(str::trim)
        .find(|s| !s.is_empty() && !s.starts_with('#'))
        .unwrap_or("")
        .chars()
        .take(180)
        .collect();
    Ok(Page {
        path,
        title,
        summary,
        tags: fm.get("tags").map(|s| vault::list(s)).unwrap_or_default(),
        revision: revision(&text),
        compiled,
        search_text: text,
    })
}
async fn catalog(State(app): State<Shared>) -> ApiResult {
    work(app, |v| {
        let board = Board::new(v);
        let entries = board
            .list(None)?
            .into_iter()
            .filter_map(|e| board.read(&e.key).ok())
            .collect::<Vec<_>>();
        let compiled = v.compiled()?;
        let mut wiki = v
            .wiki_files()?
            .into_iter()
            .map(|p| page(v, p, false))
            .collect::<Result<Vec<_>>>()?;
        // Keep the Wiki entry point first, preserving the order of other pages.
        wiki.sort_by_key(|p| p.path != "wiki/index.md");
        let raw = v
            .raw_files()?
            .into_iter()
            .map(|p| {
                let done = compiled.contains(&p);
                page(v, p, done)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(json!({"root":v.root(), "board":entries, "wiki":wiki, "raw":raw}))
    })
    .await
}
#[derive(Deserialize)]
struct PathQuery {
    path: String,
}
async fn document(State(app): State<Shared>, Query(query): Query<PathQuery>) -> ApiResult {
    work(app, move |v| {
        let text = v.read(&query.path)?;
        Ok(json!({"path":query.path,"text":text,"revision":revision(&text),"html":markdown(vault::body(&text))}))
    }).await
}
#[derive(Deserialize)]
struct BoardWrite {
    key: String,
    value: String,
    expect_version: u64,
}
async fn save_board(State(app): State<Shared>, Json(body): Json<BoardWrite>) -> ApiResult {
    work(app, move |v| {
        Ok(serde_json::to_value(Board::new(v).put(
            &body.key,
            &body.value,
            Some(body.expect_version),
        )?)?)
    })
    .await
}
#[derive(Deserialize)]
struct BoardDelete {
    key: String,
    expect_version: u64,
}
async fn delete_board(State(app): State<Shared>, Json(body): Json<BoardDelete>) -> ApiResult {
    work(app, move |v| {
        Ok(serde_json::to_value(
            Board::new(v).delete_checked(&body.key, Some(body.expect_version))?,
        )?)
    })
    .await
}
#[derive(Deserialize)]
struct PageWrite {
    path: String,
    text: String,
    expected_revision: Option<String>,
}
async fn save_document(State(app): State<Shared>, Json(body): Json<PageWrite>) -> ApiResult {
    work(app, move |v| {
        let _lock = web_lock(v)?;
        let path = writable_page(v, &body.path)?;
        check_revision(&path, body.expected_revision.as_deref())?;
        if body.text.trim().is_empty() {
            bail!("页面正文不能为空");
        }
        let parent = path.parent().context("missing parent")?;
        fs::create_dir_all(parent)?;
        let temp = parent.join(format!(
            ".web-{}-{:016x}.tmp",
            std::process::id(),
            fastrand::u64(..)
        ));
        let result = (|| -> Result<()> {
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temp)?;
            file.write_all(body.text.as_bytes())?;
            file.sync_all()?;
            // Recheck immediately before replacing, including external editor changes.
            check_revision(&path, body.expected_revision.as_deref())?;
            fs::rename(&temp, &path)?;
            Ok(())
        })();
        let _ = fs::remove_file(&temp);
        result?;
        Ok(json!({"revision":revision(&body.text)}))
    })
    .await
}
fn web_lock(v: &Vault) -> Result<fs::File> {
    if fs::symlink_metadata(v.root().join(".agent-context"))
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        bail!("state directory must not be a symlink");
    }
    let dir = v.root().join(".agent-context/tmp");
    if fs::symlink_metadata(&dir).is_ok_and(|m| m.file_type().is_symlink()) {
        bail!("state directory must not be a symlink");
    }
    fs::create_dir_all(&dir)?;
    let path = dir.join("web.lock");
    if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        bail!("lock must not be a symlink");
    }
    let f = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)?;
    f.lock()?;
    Ok(f)
}
fn writable_page(v: &Vault, rel: &str) -> Result<PathBuf> {
    let p = Path::new(rel);
    if !rel.starts_with("wiki/")
        || p.extension().and_then(|s| s.to_str()) != Some("md")
        || p.components().any(|c| !matches!(c, Component::Normal(_)))
        || rel.contains('\\')
    {
        bail!("只能编辑 wiki/ 下的 Markdown 页面");
    }
    let mut path = v.root().to_path_buf();
    for component in p.components() {
        path.push(component);
        if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            bail!("page path must not contain symlinks");
        }
    }
    Ok(path)
}
fn check_revision(path: &Path, expected: Option<&str>) -> Result<()> {
    let current = match fs::read_to_string(path) {
        Ok(s) => Some(revision(&s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    if current.as_deref() != expected {
        bail!("conflict: 页面已被其他会话修改或删除，请保留草稿并重新载入最新内容");
    }
    Ok(())
}

#[derive(Deserialize)]
struct Preview {
    text: String,
}
async fn preview(Json(body): Json<Preview>) -> Json<Value> {
    Json(json!({"html":markdown(vault::body(&body.text))}))
}
/// Raw HTML is displayed as text. Only safe link schemes are emitted.
pub fn markdown(text: &str) -> String {
    let events = Parser::new_ext(
        text,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
    )
    .map(|event| match event {
        Event::Html(s) | Event::InlineHtml(s) => Event::Text(s),
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Link {
            link_type,
            dest_url: safe_url(dest_url),
            title,
            id,
        }),
        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Image {
            link_type,
            dest_url: safe_url(dest_url),
            title,
            id,
        }),
        e => e,
    });
    let mut out = String::new();
    html::push_html(&mut out, events);
    out
}
fn safe_url(url: pulldown_cmark::CowStr<'_>) -> pulldown_cmark::CowStr<'_> {
    let normalized = url
        .chars()
        .filter(|c| !c.is_control() && !c.is_whitespace())
        .collect::<String>()
        .to_lowercase();
    if normalized.starts_with("//")
        || (normalized.contains(':')
            && !["https:", "http:", "mailto:"]
                .iter()
                .any(|s| normalized.starts_with(s)))
    {
        "".into()
    } else {
        url
    }
}

fn git_view(v: &Vault, rel: &str) -> Result<Value> {
    let path = writable_page(v, rel)?;
    let text = fs::read_to_string(&path)?;
    let tracked = !git::run(v.root(), &["ls-files", "--", rel])?.is_empty();
    let diff = if tracked {
        git::run(v.root(), &["diff", "HEAD", "--", rel])?
    } else {
        text.clone()
    };
    Ok(
        json!({"path":rel,"revision":revision(&text),"diff":diff,"new_file":!tracked,"changed":!tracked||!diff.is_empty()}),
    )
}
async fn git_preview(State(app): State<Shared>, Query(query): Query<PathQuery>) -> ApiResult {
    work(app, move |v| git_view(v, &query.path)).await
}
#[derive(Deserialize)]
struct Commit {
    path: String,
    expected_revision: String,
    message: String,
}
async fn git_commit(State(app): State<Shared>, Json(body): Json<Commit>) -> ApiResult {
    work(app, move |v| {
        let _lock = web_lock(v)?;
        let path = writable_page(v, &body.path)?;
        check_revision(&path, Some(&body.expected_revision))?;
        let message = body.message.trim();
        if message.is_empty() {
            bail!("请输入提交说明");
        }
        let view = git_view(v, &body.path)?;
        if view["changed"] == false {
            return Ok(json!({"committed":false}));
        }
        // --only leaves other staged files alone; raw/ and board/ are never included.
        if view["new_file"] == true {
            git::run(v.root(), &["add", "--intent-to-add", "--", &body.path])?;
        }
        git::run(
            v.root(),
            &["commit", "--only", "-m", message, "--", &body.path],
        )?;
        Ok(json!({"committed":true,"commit":git::run(v.root(), &["rev-parse","--short","HEAD"])?}))
    })
    .await
}
