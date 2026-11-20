//! Vault layout, raw capture, safe reads, pending tracking and commits.
//!
//! Layout:
//! ```text
//! <vault>/
//!   AGENTS.md                 personal maintainer rules
//!   raw/YYYY/MM/*.md          append-only records written by `capture`
//!   wiki/*.md                 pages maintained by the maintainer agent
//!   log.md                    one line per commit made through `agent-context commit`
//!   board/**/*.md             blackboard entries (see `board`; never committed)
//!   .agent-context/compiled.txt  raw paths already compiled into the wiki (versioned)
//!   .agent-context/pending.json  the batch returned by the last `pending` (ignored)
//! ```

use crate::{assets, git};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Local, SecondsFormat};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Component, Path, PathBuf};

pub const RAW_TYPES: &[&str] = &["decision", "finding", "howto", "pitfall", "reference"];

const STATE_DIR: &str = ".agent-context";
const COMPILED_FILE: &str = ".agent-context/compiled.txt";
const PENDING_FILE: &str = ".agent-context/pending.json";
const TMP_DIR: &str = ".agent-context/tmp";
const COMPILE_TRAILER: &str = "Agent-Context: compile";

#[derive(Debug, Clone)]
pub struct Vault {
    root: PathBuf,
}

/// Input for a new raw record.
#[derive(Debug, Clone, Default)]
pub struct Capture {
    pub title: String,
    pub conclusion: String,
    pub evidence: String,
    pub scope: String,
    pub kind: String,
    pub tags: Vec<String>,
    pub repo: Option<String>,
    pub reference: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Captured {
    pub id: String,
    pub path: String,
}

/// Parsed header of a raw record.
#[derive(Debug, Clone, Serialize)]
pub struct RawRecord {
    pub path: String,
    pub id: Option<String>,
    pub title: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub created: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommitOutcome {
    pub committed: bool,
    pub commit: Option<String>,
    pub raw_compiled: usize,
}

impl Vault {
    /// Open an existing vault. Fails if `root` is not a vault.
    pub fn open(root: &Path) -> Result<Self> {
        let root = root
            .canonicalize()
            .with_context(|| format!("vault not found: {}", root.display()))?;
        for dir in ["raw", "wiki"] {
            if !root.join(dir).is_dir() {
                bail!(
                    "not an agent-context vault (missing {dir}/): {} — create one with `agent-context init`",
                    root.display()
                );
            }
        }
        Ok(Self { root })
    }

    /// Create a new vault from the embedded template and initialise git.
    /// Returns a warning when the initial commit could not be made.
    pub fn init(dir: &Path) -> Result<(Self, Option<String>)> {
        if dir.exists() && fs::read_dir(dir)?.next().is_some() {
            bail!(
                "refusing to init: {} exists and is not empty",
                dir.display()
            );
        }
        fs::create_dir_all(dir)?;
        for (rel, body) in assets::VAULT_TEMPLATE {
            let path = dir.join(rel);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, body)?;
        }
        fs::create_dir_all(dir.join("wiki"))?;
        let vault = Self::open(dir)?;
        git::run(&vault.root, &["init", "--quiet"])?;
        git::run(&vault.root, &["add", "-A"])?;
        let warning = git::run(
            &vault.root,
            &["commit", "--quiet", "-m", "init agent-context vault"],
        )
        .err()
        .map(|e| format!("vault created but the initial commit failed: {e}"));
        Ok((vault, warning))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    // ---------------------------------------------------------------- capture

    /// Append a new raw record. Never overwrites an existing file, so any
    /// number of processes can capture concurrently.
    pub fn capture(&self, c: &Capture) -> Result<Captured> {
        let title = one_line(&c.title);
        if title.is_empty() {
            bail!("title must not be empty");
        }
        if c.conclusion.trim().is_empty() {
            bail!("conclusion must not be empty");
        }
        let kind = c.kind.trim().to_lowercase();
        if !RAW_TYPES.contains(&kind.as_str()) {
            bail!("type must be one of: {}", RAW_TYPES.join(", "));
        }
        let tags: Vec<String> = c
            .tags
            .iter()
            .map(|t| one_line(t))
            .filter(|t| !t.is_empty())
            .collect();

        let now = Local::now();
        let tmp_dir = self.root.join(TMP_DIR);
        fs::create_dir_all(&tmp_dir)?;
        let dest_dir = self
            .root
            .join("raw")
            .join(now.format("%Y").to_string())
            .join(now.format("%m").to_string());
        fs::create_dir_all(&dest_dir)?;

        for _ in 0..8 {
            let suffix = format!("{:04x}", fastrand::u16(..));
            let id = format!("{}-{suffix}", now.format("%Y%m%d-%H%M%S"));
            let name = format!("{}-{}-{suffix}.md", now.format("%Y-%m-%d"), slugify(&title));
            let dest = dest_dir.join(&name);
            let body = render_raw(&id, &title, &now, &kind, &tags, c);

            // Write fully to a private temp file, then hard-link into place:
            // the link is atomic and fails if the destination already exists.
            let tmp = tmp_dir.join(format!(
                "{id}-{}-{:016x}.tmp",
                std::process::id(),
                fastrand::u64(..)
            ));
            {
                let mut f = fs::File::create(&tmp)?;
                f.write_all(body.as_bytes())?;
                f.sync_all()?;
            }
            let linked = fs::hard_link(&tmp, &dest);
            let _ = fs::remove_file(&tmp);
            match linked {
                Ok(()) => {
                    return Ok(Captured {
                        id,
                        path: self.rel(&dest),
                    });
                }
                Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.into()),
            }
        }
        bail!("could not allocate a unique raw file name")
    }

    // ------------------------------------------------------------------ read

    /// Resolve a vault-relative path under `wiki/` or `raw/`, rejecting
    /// anything that could escape the vault.
    pub fn resolve(&self, rel: &str) -> Result<PathBuf> {
        let p = Path::new(rel.trim());
        if p.is_absolute() {
            bail!("path must be relative to the vault");
        }
        if p.components().any(|c| !matches!(c, Component::Normal(_))) {
            bail!("path must not contain '..' or '.' components");
        }
        let first = p.components().next().map(|c| c.as_os_str().to_owned());
        if !matches!(
            first.as_deref().and_then(|s| s.to_str()),
            Some("wiki" | "raw")
        ) {
            bail!("path must be under wiki/ or raw/");
        }
        if p.extension().and_then(|e| e.to_str()) != Some("md") {
            bail!("only .md files can be read");
        }
        let full = self
            .root
            .join(p)
            .canonicalize()
            .with_context(|| format!("not found: {rel}"))?;
        if !full.starts_with(&self.root) {
            bail!("path escapes the vault");
        }
        Ok(full)
    }

    pub fn read(&self, rel: &str) -> Result<String> {
        let full = self.resolve(rel)?;
        Ok(fs::read_to_string(full)?)
    }

    // ----------------------------------------------------------------- lists

    pub fn raw_files(&self) -> Result<Vec<String>> {
        self.md_files("raw")
    }

    pub fn wiki_files(&self) -> Result<Vec<String>> {
        self.md_files("wiki")
    }

    fn md_files(&self, dir: &str) -> Result<Vec<String>> {
        let mut out = Vec::new();
        walk_md(&self.root.join(dir), &mut out)?;
        let mut rel: Vec<String> = out.iter().map(|p| self.rel(p)).collect();
        rel.sort();
        Ok(rel)
    }

    /// Raw paths already compiled into the wiki.
    pub fn compiled(&self) -> Result<BTreeSet<String>> {
        match fs::read_to_string(self.root.join(COMPILED_FILE)) {
            Ok(s) => Ok(s
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(String::from)
                .collect()),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(BTreeSet::new()),
            Err(e) => Err(e.into()),
        }
    }

    /// Raw paths that have not been compiled yet.
    pub fn pending_paths(&self) -> Result<Vec<String>> {
        let compiled = self.compiled()?;
        Ok(self
            .raw_files()?
            .into_iter()
            .filter(|p| !compiled.contains(p))
            .collect())
    }

    /// List pending raw records and remember this batch, so that a later
    /// `commit(compile = true)` marks exactly these records as compiled —
    /// records captured in the meantime stay pending.
    pub fn pending(&self) -> Result<Vec<RawRecord>> {
        let paths = self.pending_paths()?;
        fs::create_dir_all(self.root.join(STATE_DIR))?;
        fs::write(
            self.root.join(PENDING_FILE),
            serde_json::to_string_pretty(&paths)?,
        )?;
        paths.iter().map(|p| self.raw_record(p)).collect()
    }

    pub fn raw_record(&self, rel: &str) -> Result<RawRecord> {
        let text = fs::read_to_string(self.root.join(rel))?;
        let fm = frontmatter(&text).unwrap_or_default();
        Ok(RawRecord {
            path: rel.to_string(),
            id: fm.get("id").map(|v| scalar(v)),
            title: fm.get("title").map(|v| scalar(v)),
            kind: fm.get("type").map(|v| scalar(v)),
            created: fm.get("created").map(|v| scalar(v)),
            tags: fm.get("tags").map(|v| list(v)).unwrap_or_default(),
        })
    }

    // ---------------------------------------------------------------- commit

    /// Commit the current vault state. With `compile`, the batch recorded by
    /// the last `pending()` is marked compiled and its raw files are
    /// committed; otherwise raw files are left out of the commit.
    pub fn commit(&self, message: &str, compile: bool) -> Result<CommitOutcome> {
        let message = one_line(message);
        if message.is_empty() {
            bail!("commit message must not be empty");
        }
        git::run(&self.root, &["add", "-A", "--", ".", ":(exclude)raw"])?;
        // The board is never versioned. New vaults ignore it in .gitignore;
        // this also covers vaults whose .gitignore predates the board. (An
        // exclude pathspec cannot be used: git rejects it for ignored paths.)
        git::run(
            &self.root,
            &[
                "rm",
                "-r",
                "-q",
                "--cached",
                "--ignore-unmatch",
                "--",
                "board",
                ".agent-context/board.lock",
            ],
        )?;

        let mut batch: Vec<String> = Vec::new();
        if compile {
            let pending_file = self.root.join(PENDING_FILE);
            let recorded: Vec<String> = match fs::read_to_string(&pending_file) {
                Ok(s) => serde_json::from_str(&s).context("corrupt .agent-context/pending.json")?,
                Err(e) if e.kind() == ErrorKind::NotFound => {
                    bail!("no pending batch recorded — run `agent-context pending` first")
                }
                Err(e) => return Err(e.into()),
            };
            let compiled = self.compiled()?;
            batch = recorded
                .into_iter()
                .filter(|p| !compiled.contains(p) && self.root.join(p).is_file())
                .collect();
        }

        if batch.is_empty() && !git::has_staged_changes(&self.root)? {
            if compile {
                let _ = fs::remove_file(self.root.join(PENDING_FILE));
            }
            return Ok(CommitOutcome {
                committed: false,
                commit: None,
                raw_compiled: 0,
            });
        }

        if !batch.is_empty() {
            let mut f = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.root.join(COMPILED_FILE))?;
            for p in &batch {
                writeln!(f, "{p}")?;
            }
            let mut args = vec!["add", "--", COMPILED_FILE];
            args.extend(batch.iter().map(String::as_str));
            git::run(&self.root, &args)?;
        }

        let stamp = Local::now().format("%Y-%m-%d %H:%M");
        let line = if compile {
            format!("- {stamp} {message} (raw: {})\n", batch.len())
        } else {
            format!("- {stamp} {message}\n")
        };
        let mut log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.root.join("log.md"))?;
        log.write_all(line.as_bytes())?;
        git::run(&self.root, &["add", "--", "log.md"])?;

        let mut args = vec!["commit", "--quiet", "-m", message.as_str()];
        let trailer;
        if compile {
            trailer = format!("{COMPILE_TRAILER}\nAgent-Context-Raw: {}", batch.len());
            args.extend(["-m", trailer.as_str()]);
        }
        git::run(&self.root, &args)?;
        if compile {
            let _ = fs::remove_file(self.root.join(PENDING_FILE));
        }
        let head = git::run(&self.root, &["rev-parse", "--short", "HEAD"])?;
        Ok(CommitOutcome {
            committed: true,
            commit: Some(head),
            raw_compiled: batch.len(),
        })
    }

    fn rel(&self, p: &Path) -> String {
        p.strip_prefix(&self.root)
            .unwrap_or(p)
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    }
}

// ------------------------------------------------------------------ helpers

fn walk_md(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let ft = entry.file_type()?;
        let path = entry.path();
        if ft.is_dir() {
            walk_md(&path, out)?;
        } else if ft.is_file() && path.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(path);
        }
    }
    Ok(())
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// File-name slug: keeps Unicode letters/digits (so CJK titles stay readable),
/// lowercases ASCII, and collapses everything else to single hyphens.
pub fn slugify(title: &str) -> String {
    let mut out = String::new();
    for ch in title.chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
        if out.chars().count() >= 48 {
            break;
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() { "note".into() } else { out }
}

/// JSON strings and arrays are valid YAML flow scalars, so serde_json does the
/// escaping for the front matter.
fn render_raw(
    id: &str,
    title: &str,
    now: &DateTime<Local>,
    kind: &str,
    tags: &[String],
    c: &Capture,
) -> String {
    let q = |s: &str| serde_json::to_string(s).expect("string serialises");
    let mut s = String::from("---\n");
    s += &format!("id: {id}\n");
    s += &format!("title: {}\n", q(title));
    s += &format!(
        "created: {}\n",
        now.to_rfc3339_opts(SecondsFormat::Secs, false)
    );
    s += &format!("type: {kind}\n");
    s += &format!(
        "tags: {}\n",
        serde_json::to_string(tags).expect("tags serialise")
    );
    if let Some(repo) = c.repo.as_deref().map(one_line).filter(|v| !v.is_empty()) {
        s += &format!("repo: {}\n", q(&repo));
    }
    if let Some(r) = c
        .reference
        .as_deref()
        .map(one_line)
        .filter(|v| !v.is_empty())
    {
        s += &format!("ref: {}\n", q(&r));
    }
    s += "---\n\n";
    s += &format!("## Conclusion\n\n{}\n\n", c.conclusion.trim());
    s += &format!("## Evidence\n\n{}\n\n", or_dash(&c.evidence));
    s += &format!("## Scope\n\n{}\n", or_dash(&c.scope));
    s
}

fn or_dash(s: &str) -> &str {
    let t = s.trim();
    if t.is_empty() { "-" } else { t }
}

/// Minimal front-matter reader: `key: value` lines between leading `---`
/// fences. Values are returned raw (see [`scalar`] / [`list`]).
pub fn frontmatter(text: &str) -> Option<BTreeMap<String, String>> {
    let rest = text.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    let mut map = BTreeMap::new();
    for line in rest[..end].lines() {
        if let Some((k, v)) = line.split_once(':') {
            let k = k.trim();
            if !k.is_empty() && !k.starts_with('#') {
                map.insert(k.to_string(), v.trim().to_string());
            }
        }
    }
    Some(map)
}

/// Body after the front matter (or the whole text when there is none).
pub fn body(text: &str) -> &str {
    text.strip_prefix("---\n")
        .and_then(|rest| rest.find("\n---").map(|end| &rest[end + 4..]))
        .map(|b| b.trim_start_matches(|c| c != '\n').trim_start_matches('\n'))
        .unwrap_or(text)
}

pub fn scalar(v: &str) -> String {
    if v.starts_with('"')
        && let Ok(s) = serde_json::from_str::<String>(v)
    {
        return s;
    }
    v.trim_matches(|c| c == '\'' || c == '"').to_string()
}

pub fn list(v: &str) -> Vec<String> {
    if let Ok(items) = serde_json::from_str::<Vec<String>>(v) {
        return items;
    }
    v.trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|t| scalar(t.trim()))
        .filter(|t| !t.is_empty())
        .collect()
}
