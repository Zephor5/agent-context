//! Structural checks: broken `[[links]]`, orphan pages, pages missing from
//! `wiki/index.md`, and malformed raw headers. Content judgement (stale or
//! duplicated pages) is left to the maintainer agent.

use crate::vault::{self, RAW_TYPES, Vault};
use anyhow::Result;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;

#[derive(Debug, Clone, Serialize)]
pub struct BrokenLink {
    pub page: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InvalidRaw {
    pub path: String,
    pub problem: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Report {
    pub ok: bool,
    pub broken_links: Vec<BrokenLink>,
    pub orphans: Vec<String>,
    pub missing_from_index: Vec<String>,
    pub invalid_raw: Vec<InvalidRaw>,
}

impl Report {
    pub fn issue_count(&self) -> usize {
        self.broken_links.len()
            + self.orphans.len()
            + self.missing_from_index.len()
            + self.invalid_raw.len()
    }
}

const INDEX: &str = "wiki/index.md";

pub fn lint(vault: &Vault) -> Result<Report> {
    let pages = vault.wiki_files()?;
    let page_set: BTreeSet<&str> = pages.iter().map(String::as_str).collect();
    // Obsidian-style resolution: a bare name matches any page with that stem.
    let mut by_stem: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for p in &pages {
        by_stem.entry(stem(p).to_lowercase()).or_default().push(p);
    }

    let mut report = Report::default();
    let mut inbound: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();

    for page in &pages {
        let text = fs::read_to_string(vault.root().join(page))?;
        for target in wikilinks(&text) {
            match resolve(vault, &target, &page_set, &by_stem) {
                Some(Resolved::Page(p)) => {
                    if p != page.as_str() {
                        inbound.entry(p).or_default().insert(page);
                    }
                }
                Some(Resolved::Other) => {}
                None => report.broken_links.push(BrokenLink {
                    page: page.clone(),
                    target,
                }),
            }
        }
    }

    for page in &pages {
        if page == INDEX {
            continue;
        }
        let from = inbound.get(page.as_str());
        if from.is_none_or(|s| s.is_empty()) {
            report.orphans.push(page.clone());
        }
        if page_set.contains(INDEX) && !from.is_some_and(|s| s.contains(INDEX)) {
            report.missing_from_index.push(page.clone());
        }
    }

    for raw in vault.raw_files()? {
        let text = fs::read_to_string(vault.root().join(&raw))?;
        if let Some(problem) = raw_problem(&text) {
            report.invalid_raw.push(InvalidRaw { path: raw, problem });
        }
    }

    report.ok = report.issue_count() == 0;
    Ok(report)
}

enum Resolved<'a> {
    Page(&'a str),
    /// A valid link to something that is not a wiki page (e.g. a raw record).
    Other,
}

fn resolve<'a>(
    vault: &Vault,
    target: &str,
    pages: &BTreeSet<&'a str>,
    by_stem: &BTreeMap<String, Vec<&'a str>>,
) -> Option<Resolved<'a>> {
    let t = target.trim().trim_end_matches(".md");
    if t.is_empty() {
        return None;
    }
    if t.starts_with("raw/") {
        return vault
            .resolve(&format!("{t}.md"))
            .ok()
            .map(|_| Resolved::Other);
    }
    let t = t.strip_prefix("wiki/").unwrap_or(t);
    if let Some(p) = pages.get(format!("wiki/{t}.md").as_str()) {
        return Some(Resolved::Page(p));
    }
    let name = t.rsplit('/').next().unwrap_or(t).to_lowercase();
    by_stem
        .get(&name)
        .and_then(|v| v.first())
        .map(|p| Resolved::Page(p))
}

/// `[[target]]`, `[[target|alias]]`, `[[target#heading]]`; skips fenced code
/// and inline code spans.
pub fn wikilinks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let line = strip_inline_code(line);
        let mut rest = line.as_str();
        while let Some(start) = rest.find("[[") {
            let after = &rest[start + 2..];
            let Some(end) = after.find("]]") else { break };
            let inner = &after[..end];
            let target = inner.split(['|', '#']).next().unwrap_or("").trim();
            if !target.is_empty() {
                out.push(target.to_string());
            }
            rest = &after[end + 2..];
        }
    }
    out
}

fn strip_inline_code(line: &str) -> String {
    let mut out = String::new();
    let mut in_code = false;
    for ch in line.chars() {
        if ch == '`' {
            in_code = !in_code;
        } else if !in_code {
            out.push(ch);
        }
    }
    out
}

fn stem(path: &str) -> &str {
    path.rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(".md")
}

fn raw_problem(text: &str) -> Option<String> {
    let Some(fm) = vault::frontmatter(text) else {
        return Some("missing front matter".into());
    };
    let mut missing: Vec<&str> = ["id", "title", "created", "type"]
        .into_iter()
        .filter(|k| fm.get(*k).is_none_or(|v| vault::scalar(v).is_empty()))
        .collect();
    if !vault::body(text).contains("## Conclusion") {
        missing.push("## Conclusion section");
    }
    if !missing.is_empty() {
        return Some(format!("missing: {}", missing.join(", ")));
    }
    let kind = vault::scalar(&fm["type"]);
    if !RAW_TYPES.contains(&kind.as_str()) {
        return Some(format!("unknown type: {kind}"));
    }
    None
}
