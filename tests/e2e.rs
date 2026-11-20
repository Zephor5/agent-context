//! End-to-end: drive the real binary over stdio JSON-RPC, and the CLI.

mod common;

use common::{TempDir, git_env};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_agent-context");

fn cli(root: &Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(BIN)
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn init_vault(tmp: &TempDir) -> std::path::PathBuf {
    git_env();
    let root = tmp.path().join("vault");
    let st = Command::new(BIN).arg("init").arg(&root).status().unwrap();
    assert!(st.success());
    root
}

struct Mcp {
    child: std::process::Child,
    out: BufReader<std::process::ChildStdout>,
    next_id: u64,
}

impl Mcp {
    fn start(root: &Path) -> Self {
        let mut child = Command::new(BIN)
            .arg("serve")
            .env("AGENT_CONTEXT_ROOT", root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let out = BufReader::new(child.stdout.take().unwrap());
        let mut m = Self {
            child,
            out,
            next_id: 1,
        };
        let init = m.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "test-client", "version": "0"}
            }),
        );
        assert_eq!(init["result"]["serverInfo"]["name"], "agent-context");
        m.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        m
    }

    fn send(&mut self, msg: Value) {
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{msg}").unwrap();
        stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let mut line = String::new();
            assert!(
                self.out.read_line(&mut line).unwrap() > 0,
                "server closed stdout"
            );
            let v: Value = serde_json::from_str(&line).unwrap();
            if v["id"] == json!(id) {
                return v;
            }
        }
    }

    /// Returns (is_error, text).
    fn call(&mut self, tool: &str, args: Value) -> (bool, String) {
        let r = self.request("tools/call", json!({"name": tool, "arguments": args}));
        let result = &r["result"];
        assert!(result.is_object(), "protocol error: {r}");
        (
            result["isError"].as_bool().unwrap_or(false),
            result["content"][0]["text"]
                .as_str()
                .unwrap_or("")
                .to_string(),
        )
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn mcp_capture_search_read() {
    let tmp = TempDir::new("mcp");
    let root = init_vault(&tmp);
    let mut mcp = Mcp::start(&root);

    let tools = mcp.request("tools/list", json!({}));
    let mut names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            "board_delete",
            "board_edit",
            "board_list",
            "board_put",
            "board_read",
            "wiki_capture",
            "wiki_read",
            "wiki_search"
        ]
    );
    let capture_schema = &tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "wiki_capture")
        .unwrap()["inputSchema"];
    assert!(capture_schema["properties"]["type"].is_object());
    assert!(capture_schema["properties"]["ref"].is_object());

    let (err, text) = mcp.call(
        "wiki_capture",
        json!({
            "title": "rmcp 3.5 工具返回 Result<String,String>",
            "conclusion": "Err 分支会变成 isError=true 的结果，而不是协议错误",
            "evidence": "rmcp src/handler/server/tool.rs",
            "scope": "rmcp 3.x",
            "type": "finding",
            "tags": ["rust", "mcp"],
            "ref": "rmcp-3.5.0"
        }),
    );
    assert!(!err, "{text}");
    let captured: Value = serde_json::from_str(&text).unwrap();
    let path = captured["path"].as_str().unwrap().to_string();
    assert!(root.join(&path).is_file());

    let (err, text) = mcp.call("wiki_search", json!({"terms": ["isError", "no-such-term"]}));
    assert!(!err);
    let hits: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(hits[0]["path"], path.as_str());
    assert_eq!(hits[0]["layer"], "raw");
    assert_eq!(hits[0]["matched"], json!(["isError"]));

    let (err, text) = mcp.call("wiki_read", json!({"path": path}));
    assert!(!err);
    assert!(text.contains("ref: \"rmcp-3.5.0\""));

    let (err, text) = mcp.call("wiki_read", json!({"path": "../../etc/passwd"}));
    assert!(err);
    assert!(text.starts_with("error:"));

    let (err, text) = mcp.call(
        "wiki_capture",
        json!({"title": "x", "conclusion": "y", "evidence": "", "scope": "", "type": "gossip"}),
    );
    assert!(err, "invalid type must be a tool error: {text}");
}

#[test]
fn mcp_board_roundtrip() {
    let tmp = TempDir::new("mcp-board");
    let root = init_vault(&tmp);
    let mut mcp = Mcp::start(&root);

    let (err, text) = mcp.call("board_list", json!({}));
    assert!(!err, "{text}");
    assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), json!([]));

    let (err, text) = mcp.call(
        "board_put",
        json!({"key": "repo:wiki:handoff", "value": "codex: 黑板实现到一半\n下一步写测试", "expect_version": 0}),
    );
    assert!(!err, "{text}");
    assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["version"], 1);

    let (err, text) = mcp.call("board_list", json!({"query": "黑板"}));
    assert!(!err);
    let list: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(list[0]["key"], "repo:wiki:handoff");
    assert_eq!(list[0]["summary"], "codex: 黑板实现到一半");

    let (err, text) = mcp.call("board_read", json!({"key": "repo:wiki:handoff"}));
    assert!(!err);
    assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["version"], 1);

    let (err, text) = mcp.call(
        "board_edit",
        json!({"key": "repo:wiki:handoff", "old_text": "下一步写测试", "new_text": "下一步等 CI", "expect_version": 1}),
    );
    assert!(!err, "{text}");
    assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["version"], 2);
    let (_, text) = mcp.call("board_read", json!({"key": "repo:wiki:handoff"}));
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap()["value"],
        "codex: 黑板实现到一半\n下一步等 CI"
    );
    let (err, text) = mcp.call(
        "board_edit",
        json!({"key": "repo:wiki:handoff", "old_text": "下一步写测试", "new_text": "x"}),
    );
    assert!(err && text.contains("not found"), "{text}");

    let (err, text) = mcp.call(
        "board_put",
        json!({"key": "repo:wiki:handoff", "value": "x", "expect_version": 0}),
    );
    assert!(err && text.contains("version conflict"), "{text}");

    let (err, text) = mcp.call("board_put", json!({"key": "../x", "value": "x"}));
    assert!(err, "{text}");

    let (err, text) = mcp.call("board_delete", json!({"key": "repo:wiki:handoff"}));
    assert!(!err);
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap()["deleted"],
        true
    );
}

#[test]
fn cli_board() {
    let tmp = TempDir::new("cli-board");
    let root = init_vault(&tmp);
    let (code, out, _) = cli(&root, &["board", "put", "env:sit", "--value", "sit 挂了"]);
    assert_eq!(code, 0, "{out}");
    let (code, out, _) = cli(&root, &["board", "list", "sit"]);
    assert_eq!(code, 0);
    assert!(out.contains("env:sit") && out.contains("sit 挂了"), "{out}");
    let (code, _, err) = cli(
        &root,
        &[
            "board",
            "put",
            "env:sit",
            "--value",
            "x",
            "--expect-version",
            "0",
        ],
    );
    assert_eq!(code, 2, "{err}");
    let (code, out, err) = cli(
        &root,
        &[
            "board",
            "edit",
            "env:sit",
            "--old",
            "挂了",
            "--new",
            "恢复了",
        ],
    );
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("version 2"), "{out}");
    let (_, out, _) = cli(&root, &["board", "read", "env:sit"]);
    assert!(out.contains("sit 恢复了"), "{out}");
    let (code, _, err) = cli(
        &root,
        &["board", "edit", "env:sit", "--old", "挂了", "--new", "x"],
    );
    assert_eq!(code, 2, "{err}");
    let (code, out, _) = cli(&root, &["board", "delete", "env:sit"]);
    assert_eq!(code, 0);
    assert!(out.contains("deleted"));
    let (_, out, _) = cli(&root, &["board", "list"]);
    assert!(out.contains("board is empty"));
}

#[test]
fn concurrent_mcp_processes_capture_without_loss() {
    let tmp = TempDir::new("mcp-procs");
    let root = init_vault(&tmp);
    let workers: Vec<_> = (0..4)
        .map(|p| {
            let root = root.clone();
            std::thread::spawn(move || {
                let mut mcp = Mcp::start(&root);
                for i in 0..20 {
                    let (err, text) = mcp.call(
                        "wiki_capture",
                        json!({"title": "same", "conclusion": format!("p{p} n{i}"),
                               "evidence": "", "scope": "", "type": "finding"}),
                    );
                    assert!(!err, "{text}");
                }
            })
        })
        .collect();
    for w in workers {
        w.join().unwrap();
    }
    let v = agent_context::Vault::open(&root).unwrap();
    let raws = v.raw_files().unwrap();
    assert_eq!(raws.len(), 80);
    let mut bodies: Vec<String> = raws
        .iter()
        .map(|r| {
            let t = std::fs::read_to_string(root.join(r)).unwrap();
            t.lines().find(|l| l.starts_with('p')).unwrap().to_string()
        })
        .collect();
    bodies.sort();
    bodies.dedup();
    assert_eq!(bodies.len(), 80, "every capture is intact and distinct");
}

#[test]
fn cli_pending_commit_lint_prompt() {
    let tmp = TempDir::new("cli");
    let root = init_vault(&tmp);

    let (code, out, _) = cli(&root, &["pending"]);
    assert_eq!(code, 0);
    assert!(out.contains("no pending"));

    std::fs::create_dir_all(root.join("raw/2026/09")).unwrap();
    std::fs::write(
        root.join("raw/2026/09/2026-09-29-x-0001.md"),
        "---\nid: 1\ntitle: \"X\"\ncreated: 2026-09-29T00:00:00+08:00\ntype: howto\ntags: []\n---\n\n## Conclusion\n\nx\n",
    )
    .unwrap();
    let (code, out, _) = cli(&root, &["pending", "--json"]);
    assert_eq!(code, 0);
    let recs: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(recs[0]["type"], "howto");

    std::fs::write(root.join("wiki/x.md"), "# X\n").unwrap();
    // x.md is not linked from the index yet → lint fails with exit 1
    let (code, out, _) = cli(&root, &["lint", "--json"]);
    assert_eq!(code, 1);
    let rep: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(rep["orphans"][0], "wiki/x.md");

    std::fs::write(root.join("wiki/index.md"), "# Index\n\n- [[x]]\n").unwrap();
    let (code, out, _) = cli(&root, &["lint"]);
    assert_eq!(code, 0, "{out}");

    let (code, out, _) = cli(
        &root,
        &["commit", "--compile", "-m", "compile: 1 raw", "--json"],
    );
    assert_eq!(code, 0);
    let c: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(c["raw_compiled"], 1);

    let (code, out, _) = cli(&root, &["prompt", "maintainer"]);
    assert_eq!(code, 0);
    assert!(out.contains("agent-context pending --json"));

    // missing root outside a vault is a clear error; inside a vault cwd is used
    let out = Command::new(BIN)
        .arg("lint")
        .env_remove("AGENT_CONTEXT_ROOT")
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--root"));
    let out = Command::new(BIN)
        .arg("lint")
        .env_remove("AGENT_CONTEXT_ROOT")
        .current_dir(&root)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn root_environment_and_explicit_override() {
    let tmp = TempDir::new("root-env");
    let root = init_vault(&tmp);
    let missing = tmp.path().join("missing");
    for mode in ["environment", "explicit"] {
        let mut command = Command::new(BIN);
        command
            .arg("lint")
            .current_dir(tmp.path())
            .env("AGENT_CONTEXT_ROOT", &root);
        if mode == "explicit" {
            command
                .arg("--root")
                .arg(&root)
                .env("AGENT_CONTEXT_ROOT", &missing);
        }
        let result = command.output().unwrap();
        assert!(
            result.status.success(),
            "{mode}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

#[test]
fn web_remembers_vault_and_starts_on_loopback() {
    let tmp = TempDir::new("web-cli");
    let root = init_vault(&tmp);
    struct Server(std::process::Child);
    impl Drop for Server {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    // The second launch remembers the vault in the new application's config.
    for mode in ["explicit", "saved"] {
        let mut command = Command::new(BIN);
        command
            .args(["web", "--no-open", "--port", "0"])
            .current_dir(tmp.path())
            .env_remove("AGENT_CONTEXT_ROOT")
            .env("XDG_CONFIG_HOME", tmp.path().join("config"))
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if mode == "explicit" {
            command.arg("--root").arg(&root);
        }
        let mut server = Server(command.spawn().unwrap());
        let mut out = BufReader::new(server.0.stdout.take().unwrap());
        let mut line = String::new();
        out.read_line(&mut line).unwrap();
        let address = line
            .trim()
            .strip_prefix("Agent Context: http://")
            .expect("startup URL");
        assert!(address.starts_with("127.0.0.1:"));
        let mut stream = std::net::TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        write!(
            stream,
            "GET /api/catalog HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        std::io::Read::read_to_string(&mut stream, &mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.contains("wiki/index.md"));
        let saved: Value = serde_json::from_slice(
            &std::fs::read(tmp.path().join("config/agent-context/web.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            saved["root"],
            root.canonicalize().unwrap().to_str().unwrap()
        );
    }
}
