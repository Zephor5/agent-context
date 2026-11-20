mod common;
use agent_context::{board::Board, git, web};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use common::{new_vault, write};
use serde_json::{Value, json};
use tower::ServiceExt;

fn app(v: &agent_context::Vault) -> Router {
    web::router(v.clone(), "127.0.0.1:7420".into())
}
async fn request(app: Router, method: &str, url: &str, body: Value) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(url)
        .header("host", "127.0.0.1:7420")
        .header("content-type", "application/json")
        .header("x-agent-context", "1")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!({"text":String::from_utf8_lossy(&bytes)})),
    )
}

#[tokio::test]
async fn board_crud_checks_concurrent_updates_and_deletes() {
    let (_tmp, v) = new_vault("web-board");
    let (status, _) = request(
        app(&v),
        "POST",
        "/api/board",
        json!({"key":"repo:test","value":"交接\n正文","expect_version":0}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, catalog) = request(app(&v), "GET", "/api/catalog", json!(null)).await;
    assert_eq!(catalog["board"][0]["version"], 1);
    Board::new(&v)
        .put("repo:test", "来自另一个 agent", Some(1))
        .unwrap();
    for method in ["POST", "DELETE"] {
        let (status, _) = request(
            app(&v),
            method,
            "/api/board",
            json!({"key":"repo:test","value":"旧草稿","expect_version":1}),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
    }
    assert_eq!(
        Board::new(&v).read("repo:test").unwrap().value,
        "来自另一个 agent"
    );
    assert_eq!(
        request(
            app(&v),
            "DELETE",
            "/api/board",
            json!({"key":"repo:test","expect_version":2})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert!(Board::new(&v).list(None).unwrap().is_empty());
}

#[tokio::test]
async fn wiki_create_edit_conflict_and_raw_are_protected() {
    let (_tmp, v) = new_vault("web-pages");
    let create = json!({"path":"wiki/中文.md","text":"---\ntags: [开发]\n---\n# 页面\n\n正文","expected_revision":null});
    assert_eq!(
        request(app(&v), "PUT", "/api/document", create.clone())
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        request(app(&v), "PUT", "/api/document", create).await.0,
        StatusCode::CONFLICT
    );
    let (_, doc) = request(
        app(&v),
        "GET",
        "/api/document?path=wiki%2F%E4%B8%AD%E6%96%87.md",
        json!(null),
    )
    .await;
    assert!(!doc["html"].as_str().unwrap().contains("tags:"));
    write(&v, "wiki/中文.md", "# 外部更新");
    assert_eq!(
        request(
            app(&v),
            "PUT",
            "/api/document",
            json!({"path":"wiki/中文.md","text":"# 旧草稿","expected_revision":doc["revision"]})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (_, catalog) = request(app(&v), "GET", "/api/catalog", json!(null)).await;
    assert!(
        catalog["wiki"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["title"] == "外部更新")
    );
    write(&v, "raw/note.md", "unchanged");
    for path in [
        "raw/note.md",
        "wiki/../raw/note.md",
        "/tmp/escape.md",
        "wiki/../../escape.md",
    ] {
        assert_eq!(
            request(
                app(&v),
                "PUT",
                "/api/document",
                json!({"path":path,"text":"bad","expected_revision":null})
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(v.read("raw/note.md").unwrap(), "unchanged");
}

#[tokio::test]
async fn two_web_writers_cannot_both_replace_a_revision() {
    let (_tmp, v) = new_vault("web-race");
    write(&v, "wiki/race.md", "base");
    let revision = web::revision("base");
    let body = |text| json!({"path":"wiki/race.md","text":text,"expected_revision":revision});
    let (a, b) = tokio::join!(
        request(app(&v), "PUT", "/api/document", body("first")),
        request(app(&v), "PUT", "/api/document", body("second"))
    );
    assert!(
        (a.0 == StatusCode::OK && b.0 == StatusCode::CONFLICT)
            || (b.0 == StatusCode::OK && a.0 == StatusCode::CONFLICT)
    );
}

#[tokio::test]
async fn git_commit_includes_only_reviewed_page() {
    let (_tmp, v) = new_vault("web-git");
    write(&v, "wiki/other.md", "unrelated staged file");
    git::run(v.root(), &["add", "--", "wiki/other.md"]).unwrap();
    write(&v, "raw/pending.md", "pending source");
    let text = "# Web edit\n\nCurrent page only";
    write(&v, "wiki/index.md", text);
    let (_, preview) = request(app(&v), "GET", "/api/git?path=wiki/index.md", json!(null)).await;
    assert_eq!(preview["changed"], true);
    let body = json!({"path":"wiki/index.md","expected_revision":preview["revision"],"message":"edit current page"});
    let (status, result) = request(app(&v), "POST", "/api/git", body).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["committed"], true);
    assert_eq!(
        git::run(
            v.root(),
            &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"]
        )
        .unwrap(),
        "wiki/index.md"
    );
    assert_eq!(
        git::run(v.root(), &["diff", "--cached", "--name-only"]).unwrap(),
        "wiki/other.md"
    );
    assert!(
        git::run(v.root(), &["ls-files", "--", "raw/pending.md"])
            .unwrap()
            .is_empty()
    );
    write(&v, "wiki/new.md", "# Newly created");
    let (status,result)=request(app(&v),"POST","/api/git",json!({"path":"wiki/new.md","expected_revision":web::revision("# Newly created"),"message":"new page"})).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(
        git::run(
            v.root(),
            &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"]
        )
        .unwrap(),
        "wiki/new.md"
    );
    write(&v, "wiki/new.md", "external");
    assert_eq!(request(app(&v),"POST","/api/git",json!({"path":"wiki/new.md","expected_revision":web::revision("# Newly created"),"message":"stale"})).await.0,StatusCode::CONFLICT);
}

#[tokio::test]
async fn loopback_http_rejects_cross_origin_and_rebinding() {
    let (_tmp, v) = new_vault("web-security");
    for (host, origin, custom) in [
        ("attacker.invalid", "http://127.0.0.1:7420", true),
        ("127.0.0.1:7420", "https://attacker.invalid", true),
        ("127.0.0.1:7420", "http://127.0.0.1:7420", false),
    ] {
        let mut req = Request::builder()
            .method("POST")
            .uri("/api/board")
            .header("host", host)
            .header("origin", origin)
            .header("content-type", "application/json");
        if custom {
            req = req.header("x-agent-context", "1");
        }
        let response = app(&v)
            .oneshot(req.body(Body::from("{}")).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    let response = app(&v)
        .oneshot(
            Request::builder()
                .uri("/")
                .header("host", "127.0.0.1:7420")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("frame-ancestors 'none'")
    );
}

#[test]
fn markdown_renders_tables_and_escapes_active_content() {
    let html = web::markdown(
        "# 标题\n\n<script>alert(1)</script>\n\n[x](javascript:alert%281%29)\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n- [x] done",
    );
    assert!(html.contains("<h1>标题</h1>"));
    assert!(html.contains("<table>"));
    assert!(html.contains("type=\"checkbox\""));
    assert!(!html.contains("<script>"));
    assert!(!html.contains("href=\"javascript:"));
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_pages_and_board_cannot_escape() {
    let (tmp, v) = new_vault("web-symlink");
    let outside = tmp.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("page.md"), "outside").unwrap();
    std::os::unix::fs::symlink(&outside, v.root().join("wiki/link")).unwrap();
    assert_eq!(request(app(&v),"PUT","/api/document",json!({"path":"wiki/link/page.md","text":"bad","expected_revision":web::revision("outside")})).await.0,StatusCode::BAD_REQUEST);
    std::os::unix::fs::symlink(&outside, v.root().join("board")).unwrap();
    assert!(Board::new(&v).put("page", "bad", None).is_err());
    assert!(Board::new(&v).list(None).is_err());
    assert_eq!(
        std::fs::read_to_string(outside.join("page.md")).unwrap(),
        "outside"
    );
}
