//! The blackboard: short-lived working state shared by every agent tool on
//! this machine — cross-tool handoffs, who is using which local resource and
//! why, environment status.
//!
//! Semantics are the opposite of `raw/`: entries are overwritten by key, have
//! no expiry, and are not versioned in git. Keys are free-form; `:` separates
//! segments and maps to directories, so `repo:harness:handoff` is stored as
//! `board/repo/harness/handoff.md` and stays readable in any editor.
//!
//! The board is a note, not a lock: it never decides whether a resource is
//! free. `expect_version` only prevents two writers from silently overwriting
//! each other's update of the same entry.

use crate::vault::{self, Vault};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, FixedOffset, Local, SecondsFormat};
use serde::Serialize;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

pub const BOARD_DIR: &str = "board";
const LOCK_FILE: &str = ".agent-context/board.lock";
const TMP_DIR: &str = ".agent-context/tmp";
const SUMMARY_CHARS: usize = 120;

/// One line in `board_list`.
#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub key: String,
    pub summary: String,
    pub updated_at: String,
}

/// A full entry, as returned by `board_read`.
#[derive(Debug, Clone, Serialize)]
pub struct Item {
    pub key: String,
    pub version: u64,
    pub updated_at: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Written {
    pub key: String,
    pub version: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Deleted {
    pub key: String,
    pub deleted: bool,
}

pub struct Board<'a> {
    vault: &'a Vault,
}

impl<'a> Board<'a> {
    pub fn new(vault: &'a Vault) -> Self {
        Self { vault }
    }

    fn dir(&self) -> PathBuf {
        self.vault.root().join(BOARD_DIR)
    }

    /// Entries whose key or value contains every whitespace-separated term of
    /// `query` (case-insensitive), newest first. No query lists everything.
    pub fn list(&self, query: Option<&str>) -> Result<Vec<Entry>> {
        if fs::symlink_metadata(self.dir()).is_ok_and(|m| m.file_type().is_symlink()) {
            bail!("board directory must not be a symlink");
        }
        let terms: Vec<String> = query
            .unwrap_or("")
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        let mut rows: Vec<(Option<DateTime<FixedOffset>>, Entry)> = Vec::new();
        let mut files = Vec::new();
        walk(&self.dir(), &mut files)?;
        for path in files {
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            let item = self.parse(&path, &text);
            if !terms.is_empty() {
                let hay = format!("{}\n{}", item.key, item.value).to_lowercase();
                if !terms.iter().all(|t| hay.contains(t.as_str())) {
                    continue;
                }
            }
            let ts = DateTime::parse_from_rfc3339(&item.updated_at).ok();
            rows.push((
                ts,
                Entry {
                    summary: summary(&item.value),
                    key: item.key,
                    updated_at: item.updated_at,
                },
            ));
        }
        rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.key.cmp(&b.1.key)));
        Ok(rows.into_iter().map(|(_, e)| e).collect())
    }

    pub fn read(&self, key: &str) -> Result<Item> {
        let key = key.trim();
        let path = self.path_of(key)?;
        match self.load(&path)? {
            Some(item) if item.key == key => Ok(item),
            Some(item) => bail!(
                "no entry {key:?} (the file belongs to {:?}; keys differing only in case share a file on case-insensitive filesystems)",
                item.key
            ),
            None => bail!("no entry {key:?}"),
        }
    }

    /// Create or overwrite `key`. With `expect_version`, the write only
    /// happens if the entry's current version matches (0 = must not exist).
    pub fn put(&self, key: &str, value: &str, expect_version: Option<u64>) -> Result<Written> {
        let key = key.trim();
        let path = self.path_of(key)?;
        let value = clean_value(value)?;
        let _lock = self.lock()?;
        let current = self.current(&path, key)?;
        let current_version = current.as_ref().map_or(0, |c| c.version);
        check_version(key, expect_version, current_version)?;
        self.write_entry(&path, key, value, current_version + 1)
    }

    /// Replace the single occurrence of `old` in `key`'s value with `new`, so
    /// a writer can change one line without resending the whole entry. The
    /// replacement is applied to the content current under the board lock;
    /// `old` matching exactly once is what makes that safe — a missing or
    /// repeated match means the entry is not what the caller last read.
    /// `expect_version` additionally refuses any intervening update.
    pub fn edit(
        &self,
        key: &str,
        old: &str,
        new: &str,
        expect_version: Option<u64>,
    ) -> Result<Written> {
        let key = key.trim();
        let path = self.path_of(key)?;
        if old.is_empty() {
            bail!("old_text must not be empty");
        }
        let _lock = self.lock()?;
        let Some(current) = self.current(&path, key)? else {
            bail!("no entry {key:?}; create it with board_put");
        };
        check_version(key, expect_version, current.version)?;
        match occurrences(&current.value, old) {
            0 => bail!(
                "old_text not found in {key:?} (version {}) — board_read it and copy the text exactly, line breaks included",
                current.version
            ),
            1 => {}
            _ => bail!(
                "old_text occurs more than once in {key:?}; include more surrounding text so it matches exactly once"
            ),
        }
        let edited = current.value.replacen(old, new, 1);
        let value = clean_value(&edited)?;
        self.write_entry(&path, key, value, current.version + 1)
    }

    pub fn delete(&self, key: &str) -> Result<Deleted> {
        self.delete_checked(key, None)
    }

    /// Delete only the revision the user reviewed, while holding the board lock.
    pub fn delete_checked(&self, key: &str, expect_version: Option<u64>) -> Result<Deleted> {
        let key = key.trim();
        let path = self.path_of(key)?;
        let _lock = self.lock()?;
        let deleted = match self.load(&path)? {
            Some(item) if item.key == key => {
                if let Some(expected) = expect_version
                    && expected != item.version
                {
                    bail!(
                        "version conflict: expected {expected}, current is {}",
                        item.version
                    );
                }
                fs::remove_file(&path)?;
                self.prune_empty_dirs(&path);
                true
            }
            _ => false,
        };
        Ok(Deleted {
            key: key.to_string(),
            deleted,
        })
    }

    // ------------------------------------------------------------ internals

    /// The entry stored at `path`, refusing a file that belongs to a key
    /// differing only in case. Call with the board lock held.
    fn current(&self, path: &Path, key: &str) -> Result<Option<Item>> {
        let current = self.load(path)?;
        if let Some(cur) = &current
            && cur.key != key
        {
            bail!(
                "key {key:?} collides with existing entry {:?} (same file on a case-insensitive filesystem); use that key or a different one",
                cur.key
            );
        }
        Ok(current)
    }

    /// Write `value` as `key` at `version`. Call with the board lock held.
    fn write_entry(&self, path: &Path, key: &str, value: &str, version: u64) -> Result<Written> {
        let now = Local::now().to_rfc3339_opts(SecondsFormat::Secs, false);
        let body = format!(
            "---\nkey: {}\nversion: {version}\nupdated_at: {now}\n---\n\n{value}\n",
            serde_json::to_string(key).expect("string serialises")
        );
        self.write_atomic(path, &body)?;
        Ok(Written {
            key: key.to_string(),
            version,
        })
    }

    /// Map a key to its file, rejecting anything that is not a plain
    /// relative name inside `board/`.
    fn path_of(&self, key: &str) -> Result<PathBuf> {
        if key.is_empty() {
            bail!("key must not be empty");
        }
        let mut path = self.dir();
        let segments: Vec<&str> = key.split(':').collect();
        for (i, seg) in segments.iter().enumerate() {
            if seg.is_empty() {
                bail!("key {key:?} has an empty segment (':' separates segments)");
            }
            if seg.starts_with('.') {
                bail!("key segments must not start with '.'");
            }
            if seg
                .chars()
                .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '/' | '\\'))
            {
                bail!("key must not contain whitespace, '/' or '\\'");
            }
            if i + 1 == segments.len() {
                path.push(format!("{seg}.md"));
            } else {
                path.push(seg);
            }
            // The Web UI is another local writer; do not follow hand-created
            // symlinks out of board/ (also protects MCP/CLI writes).
            if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
                bail!("board path must not contain symlinks");
            }
        }
        if fs::symlink_metadata(self.dir()).is_ok_and(|m| m.file_type().is_symlink()) {
            bail!("board directory must not be a symlink");
        }
        Ok(path)
    }

    fn load(&self, path: &Path) -> Result<Option<Item>> {
        match fs::read_to_string(path) {
            Ok(text) => Ok(Some(self.parse(path, &text))),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            // A directory named like the entry (key `a` next to `a:b`) is not an entry.
            Err(_) if path.is_dir() => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Parse an entry file. Files written by hand without front matter are
    /// still entries: the key comes from the path, version 1, mtime.
    fn parse(&self, path: &Path, text: &str) -> Item {
        let fm = vault::frontmatter(text).unwrap_or_default();
        let key = fm
            .get("key")
            .map(|v| vault::scalar(v))
            .filter(|k| !k.is_empty())
            .unwrap_or_else(|| self.key_from_path(path));
        let version = fm
            .get("version")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(1);
        let updated_at = fm
            .get("updated_at")
            .map(|v| vault::scalar(v))
            .unwrap_or_else(|| mtime(path));
        let value = if fm.is_empty() {
            text
        } else {
            vault::body(text)
        };
        Item {
            key,
            version,
            updated_at,
            value: value.trim_matches('\n').trim_end().to_string(),
        }
    }

    fn key_from_path(&self, path: &Path) -> String {
        let rel = path.strip_prefix(self.dir()).unwrap_or(path);
        let parts: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        parts.join(":").trim_end_matches(".md").to_string()
    }

    fn lock(&self) -> Result<fs::File> {
        let path = self.vault.root().join(LOCK_FILE);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let f = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .with_context(|| format!("open {}", path.display()))?;
        f.lock().context("lock the board")?;
        Ok(f)
    }

    fn write_atomic(&self, dest: &Path, body: &str) -> Result<()> {
        let tmp_dir = self.vault.root().join(TMP_DIR);
        fs::create_dir_all(&tmp_dir)?;
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = tmp_dir.join(format!(
            "board-{}-{:016x}.tmp",
            std::process::id(),
            fastrand::u64(..)
        ));
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(body.as_bytes())?;
            f.sync_all()?;
        }
        fs::rename(&tmp, dest).inspect_err(|_| {
            let _ = fs::remove_file(&tmp);
        })?;
        Ok(())
    }

    fn prune_empty_dirs(&self, file: &Path) {
        let root = self.dir();
        let mut dir = file.parent();
        while let Some(d) = dir {
            if d == root || !d.starts_with(&root) || fs::remove_dir(d).is_err() {
                break;
            }
            dir = d.parent();
        }
    }
}

/// Values are stored without surrounding blank lines or trailing whitespace,
/// and must keep a first line to serve as the summary.
fn clean_value(value: &str) -> Result<&str> {
    let value = value.trim_matches('\n').trim_end();
    if value.trim().is_empty() {
        bail!("value must not be empty; its first line is the entry's summary");
    }
    Ok(value)
}

fn check_version(key: &str, expected: Option<u64>, current: u64) -> Result<()> {
    if let Some(expected) = expected
        && expected != current
    {
        bail!(
            "version conflict on {key:?}: expected {expected}, current is {current} — board_read it again, merge, and retry"
        );
    }
    Ok(())
}

/// How many times `needle` occurs in `hay`, overlapping matches included,
/// counting no further than 2 (all an edit needs to know).
fn occurrences(hay: &str, needle: &str) -> usize {
    let mut count = 0;
    let mut from = 0;
    while let Some(i) = hay[from..].find(needle) {
        count += 1;
        if count == 2 {
            break;
        }
        let at = from + i;
        from = at + hay[at..].chars().next().map_or(1, char::len_utf8);
    }
    count
}

fn summary(value: &str) -> String {
    let line = value
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let mut s: String = line.chars().take(SUMMARY_CHARS).collect();
    if line.chars().count() > SUMMARY_CHARS {
        s.push('…');
    }
    s
}

fn mtime(path: &Path) -> String {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|t| DateTime::<Local>::from(t).to_rfc3339_opts(SecondsFormat::Secs, false))
        .unwrap_or_default()
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    for entry in entries {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let ft = entry.file_type()?;
        let path = entry.path();
        if ft.is_dir() {
            walk(&path, out)?;
        } else if ft.is_file() && path.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(path);
        }
    }
    Ok(())
}
