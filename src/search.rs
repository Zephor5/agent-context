//! In-process keyword search over `wiki/` pages and not-yet-compiled `raw/`
//! records. Deliberately simple and dependency-free; the MCP interface does
//! not change if this is later swapped for a BM25 or hybrid backend.

use crate::vault::{self, Vault};
use anyhow::Result;
use serde::Serialize;
use std::fs;

#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    pub path: String,
    /// `wiki` for compiled pages, `raw` for records not yet compiled.
    pub layer: &'static str,
    pub title: String,
    pub score: usize,
    /// The search terms this document contains, as the caller passed them.
    pub matched: Vec<String>,
    pub snippet: String,
}

/// Each term is one case-insensitive substring, spaces included, so a term
/// can be a phrase. CJK text works without segmentation because matching is
/// by substring. A document is returned when it contains ANY term; documents
/// containing more terms rank first, then by occurrences, with hits in the
/// title or path weighing more. One call can therefore carry every synonym
/// or alternative phrasing the caller would otherwise try one by one.
pub fn search(vault: &Vault, terms: &[String], limit: usize) -> Result<Vec<Hit>> {
    // (as passed, lowercased); blanks and case-insensitive duplicates dropped
    let mut wanted: Vec<(String, String)> = Vec::new();
    for t in terms {
        let t = t.trim();
        let low = t.to_lowercase();
        if !t.is_empty() && !wanted.iter().any(|(_, l)| *l == low) {
            wanted.push((t.to_string(), low));
        }
    }
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let lowered: Vec<String> = wanted.iter().map(|(_, l)| l.clone()).collect();

    let mut docs: Vec<(String, &'static str)> = vault
        .wiki_files()?
        .into_iter()
        .map(|p| (p, "wiki"))
        .collect();
    docs.extend(vault.pending_paths()?.into_iter().map(|p| (p, "raw")));

    let mut hits = Vec::new();
    for (path, layer) in docs {
        let Ok(text) = fs::read_to_string(vault.root().join(&path)) else {
            continue;
        };
        let title = title_of(&path, &text);
        let hay = text.to_lowercase();
        let head = format!("{} {}", title.to_lowercase(), path.to_lowercase());

        let mut score = 0;
        let mut matched = Vec::new();
        for (orig, t) in &wanted {
            let n = hay.matches(t.as_str()).count();
            let in_head = head.contains(t.as_str());
            if n > 0 || in_head {
                matched.push(orig.clone());
            }
            score += n + if in_head { 5 } else { 0 };
        }
        if matched.is_empty() {
            continue;
        }
        hits.push(Hit {
            snippet: snippet(&text, &lowered),
            path,
            layer,
            title,
            score,
            matched,
        });
    }
    // Covering more terms always ranks first, however often another document
    // repeats a single term; occurrences only order documents within a tier.
    hits.sort_by(|a, b| {
        b.matched
            .len()
            .cmp(&a.matched.len())
            .then_with(|| b.score.cmp(&a.score))
            .then_with(|| a.path.cmp(&b.path))
    });
    hits.truncate(limit.max(1));
    Ok(hits)
}

/// Front-matter `title`, else the first `# ` heading, else the file stem.
pub fn title_of(path: &str, text: &str) -> String {
    if let Some(fm_title) = vault::frontmatter(text).and_then(|fm| fm.get("title").cloned()) {
        return vault::scalar(&fm_title);
    }
    for line in vault::body(text).lines() {
        if let Some(h) = line.strip_prefix("# ") {
            return h.trim().to_string();
        }
    }
    path.rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(".md")
        .to_string()
}

fn snippet(text: &str, terms: &[String]) -> String {
    let line = vault::body(text)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .find(|l| {
            let low = l.to_lowercase();
            terms.iter().any(|t| low.contains(t.as_str()))
        })
        .or_else(|| {
            vault::body(text)
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty() && !l.starts_with('#'))
        })
        .unwrap_or("");
    let mut s: String = line.chars().take(200).collect();
    if line.chars().count() > 200 {
        s.push('…');
    }
    s
}
