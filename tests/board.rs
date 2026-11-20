mod common;

use agent_context::board::Board;
use common::{new_vault, write};

#[test]
fn put_read_list_delete() {
    let (_tmp, vault) = new_vault("board-basic");
    let b = Board::new(&vault);
    assert!(b.list(None).unwrap().is_empty());

    let w = b
        .put(
            "repo:harness:handoff",
            "claude-code 做到一半：下一步补测试\n\n分支 feat/x",
            None,
        )
        .unwrap();
    assert_eq!(w.version, 1);
    assert!(vault.root().join("board/repo/harness/handoff.md").is_file());

    let item = b.read("repo:harness:handoff").unwrap();
    assert_eq!(item.version, 1);
    assert!(item.value.ends_with("分支 feat/x"));
    assert!(!item.updated_at.is_empty());

    b.put("env:sit", "sit 挂了，网关 502", None).unwrap();
    let all = b.list(None).unwrap();
    assert_eq!(all.len(), 2);
    let h = all
        .iter()
        .find(|e| e.key == "repo:harness:handoff")
        .unwrap();
    assert_eq!(h.summary, "claude-code 做到一半：下一步补测试");

    // keywords match key or value, all terms required, case-insensitive
    assert_eq!(b.list(Some("SIT 502")).unwrap().len(), 1);
    assert_eq!(b.list(Some("harness")).unwrap().len(), 1);
    assert_eq!(b.list(Some("feat/x")).unwrap().len(), 1);
    assert!(b.list(Some("sit feat/x")).unwrap().is_empty());

    // overwrite bumps the version
    assert_eq!(b.put("env:sit", "sit 恢复", None).unwrap().version, 2);
    assert_eq!(b.read("env:sit").unwrap().value, "sit 恢复");

    assert!(b.delete("repo:harness:handoff").unwrap().deleted);
    assert!(!b.delete("repo:harness:handoff").unwrap().deleted);
    assert!(
        !vault.root().join("board/repo").exists(),
        "empty dirs pruned"
    );
    assert!(b.read("repo:harness:handoff").is_err());
}

#[test]
fn expect_version_guards_concurrent_updates() {
    let (_tmp, vault) = new_vault("board-cas");
    let b = Board::new(&vault);
    assert_eq!(b.put("k", "v1", Some(0)).unwrap().version, 1);
    let e = b.put("k", "again", Some(0)).unwrap_err().to_string();
    assert!(e.contains("version conflict"), "{e}");
    assert_eq!(b.put("k", "v2", Some(1)).unwrap().version, 2);
    assert!(b.put("k", "stale", Some(1)).is_err());
    assert_eq!(b.read("k").unwrap().value, "v2");
}

#[test]
fn concurrent_writers_with_expect_version_never_lose_updates() {
    let (_tmp, vault) = new_vault("board-race");
    Board::new(&vault).put("counter", "0", None).unwrap();
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let vault = vault.clone();
            std::thread::spawn(move || {
                let b = Board::new(&vault);
                let mut done = 0;
                while done < 25 {
                    let cur = b.read("counter").unwrap();
                    let n: u64 = cur.value.parse().unwrap();
                    if b.put("counter", &(n + 1).to_string(), Some(cur.version))
                        .is_ok()
                    {
                        done += 1;
                    }
                }
            })
        })
        .collect();
    for w in workers {
        w.join().unwrap();
    }
    let item = Board::new(&vault).read("counter").unwrap();
    assert_eq!(item.value, "100");
    assert_eq!(item.version, 101);
}

#[test]
fn keys_are_validated() {
    let (_tmp, vault) = new_vault("board-keys");
    let b = Board::new(&vault);
    for bad in [
        "", "a::b", ":a", "a:", "..:x", ".hidden", "a/b", "a\\b", "a b",
    ] {
        assert!(b.put(bad, "v", None).is_err(), "{bad:?} must be rejected");
    }
    assert!(b.put("ok", "  \n ", None).is_err(), "empty value rejected");
    // a key and a longer key under it coexist
    b.put("a", "top", None).unwrap();
    b.put("a:b", "nested", None).unwrap();
    assert_eq!(b.read("a").unwrap().value, "top");
    assert_eq!(b.read("a:b").unwrap().value, "nested");
    assert_eq!(b.list(None).unwrap().len(), 2);
}

#[test]
fn case_folding_collision_is_refused_not_merged() {
    let (_tmp, vault) = new_vault("board-case");
    let b = Board::new(&vault);
    b.put("Port:8080", "dev-stack", None).unwrap();
    let case_insensitive_fs = vault.root().join("board/port/8080.md").exists();
    if case_insensitive_fs {
        let e = b.put("port:8080", "other", None).unwrap_err().to_string();
        assert!(e.contains("collides"), "{e}");
        assert!(b.read("port:8080").is_err());
        assert!(!b.delete("port:8080").unwrap().deleted);
    } else {
        b.put("port:8080", "other", None).unwrap();
        assert_eq!(b.list(None).unwrap().len(), 2);
    }
    assert_eq!(b.read("Port:8080").unwrap().value, "dev-stack");
}

#[test]
fn hand_written_files_are_entries() {
    let (_tmp, vault) = new_vault("board-hand");
    write(&vault, "board/note/today.md", "手写的一条\n详情");
    let b = Board::new(&vault);
    let item = b.read("note:today").unwrap();
    assert_eq!(item.version, 1);
    assert_eq!(item.value, "手写的一条\n详情");
    assert_eq!(b.list(None).unwrap()[0].summary, "手写的一条");
    assert_eq!(b.put("note:today", "改过", Some(1)).unwrap().version, 2);
}

#[test]
fn board_is_never_committed() {
    for old_gitignore in [false, true] {
        let (_tmp, vault) = new_vault("board-git");
        if old_gitignore {
            // a vault created before the board existed has no ignore rule for it
            write(
                &vault,
                ".gitignore",
                ".agent-context/pending.json\n.agent-context/tmp/\n",
            );
        }
        Board::new(&vault).put("env:sit", "down", None).unwrap();
        write(&vault, "wiki/index.md", "# Index\n\nchanged\n");
        let out = vault
            .commit("edit index", false)
            .unwrap_or_else(|e| panic!("old_gitignore={old_gitignore}: {e:#}"));
        assert!(out.committed);
        let tracked = agent_context::git::run(vault.root(), &["ls-files"]).unwrap();
        assert!(!tracked.contains("board/"), "{tracked}");
        assert!(!tracked.contains("board.lock"), "{tracked}");
    }
}

#[test]
fn edit_replaces_exactly_one_occurrence() {
    let (_tmp, vault) = new_vault("board-edit");
    let b = Board::new(&vault);
    b.put(
        "repo:x:handoff",
        "codex：实现中\n\n- 分支 feat/x\n- 下一步：补测试",
        None,
    )
    .unwrap();

    // summary line and a body line, each without resending the rest
    let w = b
        .edit("repo:x:handoff", "codex：实现中", "codex：待评审", Some(1))
        .unwrap();
    assert_eq!(w.version, 2);
    b.edit("repo:x:handoff", "下一步：补测试", "下一步：等 CI", None)
        .unwrap();
    let item = b.read("repo:x:handoff").unwrap();
    assert_eq!(item.version, 3);
    assert_eq!(
        item.value,
        "codex：待评审\n\n- 分支 feat/x\n- 下一步：等 CI"
    );
    assert_eq!(b.list(None).unwrap()[0].summary, "codex：待评审");

    // append = replace the last line with itself plus the new lines
    b.edit(
        "repo:x:handoff",
        "- 下一步：等 CI",
        "- 下一步：等 CI\n- PR #12",
        None,
    )
    .unwrap();
    assert!(
        b.read("repo:x:handoff")
            .unwrap()
            .value
            .ends_with("等 CI\n- PR #12")
    );

    // empty replacement deletes; surrounding blank lines are normalised
    b.edit("repo:x:handoff", "\n- PR #12", "", None).unwrap();
    assert!(b.read("repo:x:handoff").unwrap().value.ends_with("等 CI"));
}

#[test]
fn edit_refuses_ambiguous_missing_and_stale() {
    let (_tmp, vault) = new_vault("board-edit-refuse");
    let b = Board::new(&vault);
    b.put("k", "状态：a\n- todo\n- todo", None).unwrap();

    let e = b
        .edit("k", "- todo", "- done", None)
        .unwrap_err()
        .to_string();
    assert!(e.contains("more than once"), "{e}");
    // overlapping matches are ambiguous too
    b.put("o", "aaa", None).unwrap();
    assert!(b.edit("o", "aa", "b", None).is_err());

    let e = b.edit("k", "不存在", "x", None).unwrap_err().to_string();
    assert!(e.contains("not found") && e.contains("version 1"), "{e}");
    assert!(b.edit("k", "", "x", None).is_err(), "empty old_text");
    let e = b
        .edit("k", "状态：a", "x", Some(0))
        .unwrap_err()
        .to_string();
    assert!(e.contains("version conflict"), "{e}");
    let e = b.edit("missing", "a", "b", None).unwrap_err().to_string();
    assert!(e.contains("board_put"), "{e}");
    let e = b
        .edit("k", "状态：a\n- todo\n- todo", " \n", None)
        .unwrap_err()
        .to_string();
    assert!(e.contains("must not be empty"), "{e}");

    // nothing above changed the entry
    let item = b.read("k").unwrap();
    assert_eq!(item.version, 1);
    assert_eq!(item.value, "状态：a\n- todo\n- todo");
}

#[test]
fn concurrent_edits_of_different_lines_are_all_kept() {
    let (_tmp, vault) = new_vault("board-edit-race");
    let lines: Vec<String> = (0..4).map(|i| format!("worker{i}: 0")).collect();
    Board::new(&vault)
        .put("status", &format!("状态\n{}", lines.join("\n")), None)
        .unwrap();
    let workers: Vec<_> = (0..4)
        .map(|i| {
            let vault = vault.clone();
            std::thread::spawn(move || {
                let b = Board::new(&vault);
                for n in 0..10 {
                    b.edit(
                        "status",
                        &format!("worker{i}: {n}\n"),
                        &format!("worker{i}: {}\n", n + 1),
                        None,
                    )
                    .or_else(|_| {
                        // the last line has no trailing newline
                        b.edit(
                            "status",
                            &format!("worker{i}: {n}"),
                            &format!("worker{i}: {}", n + 1),
                            None,
                        )
                    })
                    .unwrap();
                }
            })
        })
        .collect();
    for w in workers {
        w.join().unwrap();
    }
    let item = Board::new(&vault).read("status").unwrap();
    for i in 0..4 {
        assert!(
            item.value.contains(&format!("worker{i}: 10")),
            "{}",
            item.value
        );
    }
    assert_eq!(item.version, 41);
}
