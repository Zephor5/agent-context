#![allow(dead_code)]

use agent_context::Vault;
use std::path::{Path, PathBuf};
use std::sync::Once;

static ENV: Once = Once::new();

/// Deterministic git identity for commits made by the tests.
pub fn git_env() {
    ENV.call_once(|| unsafe {
        for (k, v) in [
            ("GIT_AUTHOR_NAME", "agent-context-test"),
            ("GIT_AUTHOR_EMAIL", "test@example.invalid"),
            ("GIT_COMMITTER_NAME", "agent-context-test"),
            ("GIT_COMMITTER_EMAIL", "test@example.invalid"),
            ("GIT_CONFIG_COUNT", "1"),
            ("GIT_CONFIG_KEY_0", "commit.gpgsign"),
            ("GIT_CONFIG_VALUE_0", "false"),
        ] {
            std::env::set_var(k, v);
        }
    });
}

/// A temporary directory removed on drop.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!(
            "agent-context-test-{tag}-{}-{:08x}",
            std::process::id(),
            fastrand::u32(..)
        ));
        std::fs::create_dir_all(&p).expect("create temp dir");
        Self(p)
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A fresh vault inside a temp dir.
pub fn new_vault(tag: &str) -> (TempDir, Vault) {
    git_env();
    let tmp = TempDir::new(tag);
    let (vault, warning) = Vault::init(&tmp.path().join("vault")).expect("init vault");
    assert!(warning.is_none(), "init commit failed: {warning:?}");
    (tmp, vault)
}

pub fn write(vault: &Vault, rel: &str, body: &str) {
    let p = vault.root().join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}
