mod common;

use agent_context::vault::{Capture, frontmatter, scalar, slugify};
use agent_context::{git, lint, search};
use common::{new_vault, write};
use std::sync::Arc;

fn note(title: &str) -> Capture {
    Capture {
        title: title.into(),
        conclusion: "The conclusion.".into(),
        evidence: "See src/main.rs".into(),
        scope: "Always.".into(),
        kind: "finding".into(),
        tags: vec!["t1".into()],
        repo: Some("my-repo".into()),
        reference: Some("abc123".into()),
    }
}

#[test]
fn init_creates_layout_and_commit() {
    let (_tmp, v) = new_vault("init");
    for rel in [
        "AGENTS.md",
        "log.md",
        "wiki/index.md",
        ".agent-context/compiled.txt",
        ".gitignore",
    ] {
        assert!(v.root().join(rel).is_file(), "{rel} missing");
    }
    assert!(v.root().join("raw").is_dir());
    let log = git::run(v.root(), &["log", "--oneline"]).unwrap();
    assert_eq!(log.lines().count(), 1);
}

#[test]
fn init_refuses_non_empty_dir() {
    let (_tmp, v) = new_vault("init-nonempty");
    assert!(agent_context::Vault::init(v.root()).is_err());
}

#[test]
fn open_rejects_non_vault() {
    let tmp = common::TempDir::new("notvault");
    assert!(agent_context::Vault::open(tmp.path()).is_err());
}

#[test]
fn capture_writes_valid_record() {
    let (_tmp, v) = new_vault("capture");
    let c = v.capture(&note("Auth token \"refresh\"\nrace")).unwrap();
    assert!(c.path.starts_with("raw/"));
    let text = std::fs::read_to_string(v.root().join(&c.path)).unwrap();
    let fm = frontmatter(&text).unwrap();
    assert_eq!(scalar(&fm["id"]), c.id);
    assert_eq!(scalar(&fm["title"]), "Auth token \"refresh\" race");
    assert_eq!(scalar(&fm["type"]), "finding");
    assert_eq!(scalar(&fm["repo"]), "my-repo");
    assert_eq!(scalar(&fm["ref"]), "abc123");
    assert!(text.contains("## Conclusion\n\nThe conclusion."));
    assert!(lint::lint(&v).unwrap().invalid_raw.is_empty());
    // no temp files left behind
    let tmp_dir = v.root().join(".agent-context/tmp");
    assert_eq!(std::fs::read_dir(tmp_dir).unwrap().count(), 0);
}

#[test]
fn capture_validates_input() {
    let (_tmp, v) = new_vault("capture-invalid");
    let mut c = note("x");
    c.kind = "rumour".into();
    assert!(v.capture(&c).is_err());
    let mut c = note("  ");
    c.kind = "finding".into();
    assert!(v.capture(&c).is_err());
    let mut c = note("x");
    c.conclusion = " ".into();
    assert!(v.capture(&c).is_err());
}

#[test]
fn slug_keeps_cjk() {
    assert_eq!(slugify("认证 Token 刷新!"), "认证-token-刷新");
    assert_eq!(slugify("!!!"), "note");
}

#[test]
fn concurrent_captures_do_not_collide() {
    let (_tmp, v) = new_vault("concurrent");
    let v = Arc::new(v);
    let threads: Vec<_> = (0..8)
        .map(|i| {
            let v = Arc::clone(&v);
            std::thread::spawn(move || {
                (0..25)
                    .map(|j| {
                        v.capture(&note(&format!("same title {i} {j}")))
                            .unwrap()
                            .path
                    })
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    let mut paths: Vec<String> = threads
        .into_iter()
        .flat_map(|t| t.join().unwrap())
        .collect();
    paths.sort();
    paths.dedup();
    assert_eq!(paths.len(), 200);
    assert_eq!(v.raw_files().unwrap().len(), 200);
}

#[test]
fn identical_titles_in_same_second_get_distinct_files() {
    let (_tmp, v) = new_vault("same-title");
    let a = v.capture(&note("dup")).unwrap();
    let b = v.capture(&note("dup")).unwrap();
    assert_ne!(a.path, b.path);
    assert_ne!(a.id, b.id);
}

#[test]
fn read_is_confined_to_wiki_and_raw() {
    let (tmp, v) = new_vault("read");
    assert!(v.read("wiki/index.md").is_ok());
    for bad in [
        "../x.md",
        "wiki/../AGENTS.md",
        "AGENTS.md",
        "/etc/hosts",
        "wiki/index.txt",
        "./wiki/index.md",
        "wiki/missing.md",
    ] {
        assert!(v.read(bad).is_err(), "{bad} should be rejected");
    }
    // a symlink pointing outside the vault is rejected
    let outside = tmp.path().join("secret.md");
    std::fs::write(&outside, "secret").unwrap();
    std::os::unix::fs::symlink(&outside, v.root().join("wiki/link.md")).unwrap();
    assert!(v.read("wiki/link.md").is_err());
}

#[test]
fn pending_and_compile_cycle() {
    let (_tmp, v) = new_vault("compile");
    let a = v.capture(&note("first")).unwrap();
    let b = v.capture(&note("second")).unwrap();

    let batch = v.pending().unwrap();
    assert_eq!(batch.len(), 2);
    assert_eq!(batch[0].kind.as_deref(), Some("finding"));

    // captured after `pending`: must stay pending after the compile commit
    let late = v.capture(&note("late")).unwrap();
    write(&v, "wiki/topic.md", "# Topic\n\nMerged.\n");

    let out = v.commit("compile: 2 raw → topic", true).unwrap();
    assert!(out.committed);
    assert_eq!(out.raw_compiled, 2);

    let still: Vec<String> = v.pending_paths().unwrap();
    assert_eq!(still, vec![late.path.clone()]);
    let compiled = v.compiled().unwrap();
    assert!(compiled.contains(&a.path) && compiled.contains(&b.path));

    // the late raw is not in the commit; the compiled ones and the page are
    let tracked = git::run(v.root(), &["ls-files"]).unwrap();
    assert!(tracked.contains(&a.path) && tracked.contains("wiki/topic.md"));
    assert!(!tracked.contains(&late.path));
    let msg = git::run(v.root(), &["log", "-1", "--format=%B"]).unwrap();
    assert!(msg.contains("Agent-Context: compile"));
    let log = std::fs::read_to_string(v.root().join("log.md")).unwrap();
    assert!(log.contains("compile: 2 raw → topic (raw: 2)"));
    assert!(!v.root().join(".agent-context/pending.json").exists());

    // a second compile without a new `pending` is refused
    assert!(v.commit("again", true).is_err());
}

#[test]
fn plain_commit_leaves_raw_pending() {
    let (_tmp, v) = new_vault("plain-commit");
    let r = v.capture(&note("r")).unwrap();
    write(&v, "wiki/page.md", "# Page\n");
    let out = v.commit("edit page", false).unwrap();
    assert!(out.committed);
    assert_eq!(out.raw_compiled, 0);
    assert_eq!(v.pending_paths().unwrap(), vec![r.path]);
    // nothing changed → nothing to commit, log untouched
    let log_before = std::fs::read_to_string(v.root().join("log.md")).unwrap();
    let out = v.commit("noop", false).unwrap();
    assert!(!out.committed);
    assert_eq!(
        std::fs::read_to_string(v.root().join("log.md")).unwrap(),
        log_before
    );
}

#[test]
fn search_covers_wiki_and_pending_raw_only() {
    let (_tmp, v) = new_vault("search");
    write(&v, "wiki/auth.md", "# 认证\n\nToken 刷新有竞态。\n");
    let raw = v
        .capture(&Capture {
            title: "Token refresh race".into(),
            conclusion: "刷新 token 时要加锁".into(),
            kind: "pitfall".into(),
            ..Default::default()
        })
        .unwrap();

    let hits = search::search(&v, &t(&["token"]), 10).unwrap();
    let layers: Vec<(&str, &str)> = hits.iter().map(|h| (h.path.as_str(), h.layer)).collect();
    assert!(layers.contains(&("wiki/auth.md", "wiki")));
    assert!(layers.contains(&(raw.path.as_str(), "raw")));

    let cjk = search::search(&v, &t(&["竞态"]), 10).unwrap();
    assert_eq!(cjk.len(), 1);
    assert_eq!(cjk[0].title, "认证");
    assert!(cjk[0].snippet.contains("竞态"));

    // once compiled, the raw record is no longer returned
    v.pending().unwrap();
    v.commit("compile", true).unwrap();
    let hits = search::search(&v, &t(&["token"]), 10).unwrap();
    assert!(hits.iter().all(|h| h.layer == "wiki"));

    assert!(search::search(&v, &t(&["   ", ""]), 10).unwrap().is_empty());
    assert!(search::search(&v, &[], 10).unwrap().is_empty());
}

fn t(terms: &[&str]) -> Vec<String> {
    terms.iter().map(|s| s.to_string()).collect()
}

#[test]
fn search_takes_many_terms_in_one_call() {
    let (_tmp, v) = new_vault("search-terms");
    write(&v, "wiki/auth.md", "# 认证\n\nToken 刷新有竞态。\n");
    // repeats one term many times but covers only that one
    write(
        &v,
        "wiki/other.md",
        "# Other\n\ntoken token token token token token\n",
    );
    write(
        &v,
        "wiki/err.md",
        "# Errors\n\nReturn a tool error, not a protocol error.\n",
    );

    // any term matches; covering more terms beats repeating one
    let hits = search::search(&v, &t(&["Token", "竞态", "nothing-here"]), 10).unwrap();
    let paths: Vec<&str> = hits.iter().map(|h| h.path.as_str()).collect();
    assert_eq!(paths, ["wiki/auth.md", "wiki/other.md"]);
    assert_eq!(hits[0].matched, ["Token", "竞态"]);
    assert_eq!(hits[1].matched, ["Token"]);

    // a term with spaces is one phrase, not two words
    let hits = search::search(&v, &t(&["tool error"]), 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "wiki/err.md");
    assert!(
        search::search(&v, &t(&["error tool"]), 10)
            .unwrap()
            .is_empty()
    );

    // case-insensitive duplicates count once
    let hits = search::search(&v, &t(&["token", "TOKEN"]), 10).unwrap();
    assert!(hits.iter().all(|h| h.matched == ["token"]));
}

#[test]
fn lint_reports_structural_issues() {
    let (_tmp, v) = new_vault("lint");
    assert!(lint::lint(&v).unwrap().ok);

    let raw = v.capture(&note("src")).unwrap();
    let raw_link = raw.path.trim_end_matches(".md").to_string();
    write(
        &v,
        "wiki/index.md",
        "# Index\n\n- [[a]]\n- [[wiki/b|B page]]\n",
    );
    write(
        &v,
        "wiki/a.md",
        &format!(
            "# A\n\nSee [[b#section]] and [[missing]].\n\n`[[not-a-link]]`\n\n## Sources\n\n- [[{raw_link}]]\n- [[raw/2026/01/nope]]\n"
        ),
    );
    write(&v, "wiki/b.md", "# B\n\n```\n[[in-code]]\n```\n");
    write(&v, "wiki/lonely.md", "# Lonely\n");
    write(&v, "raw/2026/01/bad.md", "no front matter\n");
    write(
        &v,
        "raw/2026/01/badtype.md",
        "---\nid: x\ntitle: \"t\"\ncreated: now\ntype: rumour\n---\n\n## Conclusion\n\nx\n",
    );

    let r = lint::lint(&v).unwrap();
    assert!(!r.ok);
    let broken: Vec<&str> = r.broken_links.iter().map(|l| l.target.as_str()).collect();
    assert_eq!(broken, vec!["missing", "raw/2026/01/nope"]);
    assert_eq!(r.orphans, vec!["wiki/lonely.md"]);
    assert_eq!(r.missing_from_index, vec!["wiki/lonely.md"]);
    let invalid: Vec<&str> = r.invalid_raw.iter().map(|i| i.path.as_str()).collect();
    assert_eq!(
        invalid,
        vec!["raw/2026/01/bad.md", "raw/2026/01/badtype.md"]
    );
    assert!(r.invalid_raw[1].problem.contains("rumour"));
}
