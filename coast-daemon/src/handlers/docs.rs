/// Handlers for `coast docs` and `coast search-docs`.
///
/// Uses embedded docs markdown and per-locale embedded search indexes so
/// packaged binaries can serve docs/search without repository files at runtime.
use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::Deserialize;
use tracing::info;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{
    DocsNode, DocsNodeKind, DocsRequest, DocsResponse, SearchDocsRequest, SearchDocsResponse,
    SearchDocsResult,
};

use crate::docs_assets::{DocsAssets, SearchIndexAssets};
use crate::server::AppState;

const SEARCH_DEFAULT_LIMIT: usize = 10;
const SEARCH_MAX_LIMIT: usize = 50;
const BM25_K1: f64 = 1.5;
const BM25_B: f64 = 0.75;
const SEMANTIC_BOOST_WEIGHT: f64 = 0.3;
const LOCALES: [&str; 6] = ["zh", "ja", "ko", "ru", "pt", "es"];
const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "in", "on", "at", "to", "for", "of", "with", "by", "is",
    "are", "was", "were", "be", "been", "being", "have", "has", "had", "do", "does", "did", "will",
    "would", "shall", "should", "may", "might", "can", "could", "this", "that", "these", "those",
    "it", "its", "not", "no", "nor", "so", "if", "then", "than", "from", "up", "out", "as", "into",
    "about", "each", "which", "their", "there", "your", "you", "we", "they", "he", "she", "de",
    "la", "el", "en", "es", "un", "una", "los", "las", "del", "por", "con",
];

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchIndexData {
    locale: String,
    sections: Vec<SearchSection>,
    inverted_index: HashMap<String, Vec<InvEntry>>,
    idf: HashMap<String, f64>,
    semantic_neighbors: HashMap<String, Vec<Neighbor>>,
    avg_dl: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchSection {
    file_path: String,
    heading: String,
    content: String,
    route: String,
    token_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct InvEntry {
    s: usize,
    tf: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct Neighbor {
    s: usize,
    score: f64,
}

#[derive(Default)]
struct DirTree {
    files: BTreeSet<String>,
    dirs: BTreeMap<String, DirTree>,
}

impl DirTree {
    fn insert_file(&mut self, rel_path: &str) {
        let mut parts = rel_path.split('/').peekable();
        self.insert_parts(&mut parts);
    }

    fn insert_parts<'a, I>(&mut self, parts: &mut std::iter::Peekable<I>)
    where
        I: Iterator<Item = &'a str>,
    {
        let Some(part) = parts.next() else {
            return;
        };
        if parts.peek().is_none() {
            self.files.insert(part.to_string());
            return;
        }
        self.dirs
            .entry(part.to_string())
            .or_default()
            .insert_parts(parts);
    }
}

/// Handle a docs request (tree and optional markdown content).
pub async fn handle_docs(req: DocsRequest, state: &AppState) -> Result<DocsResponse> {
    let locale = resolve_locale(req.language.as_deref(), &state.language());
    let localized_docs = load_docs_for_locale(&locale)?;
    let english_docs = if locale == "en" {
        HashMap::new()
    } else {
        load_docs_for_locale("en")?
    };

    let tree_source = if localized_docs.is_empty() {
        &english_docs
    } else {
        &localized_docs
    };
    let tree = build_docs_tree(tree_source.keys().cloned());

    if let Some(path) = req.path {
        let resolved =
            resolve_markdown_path(&path, &localized_docs, &english_docs).ok_or_else(|| {
                CoastError::state(format!(
                    "docs path '{path}' not found for locale '{locale}'"
                ))
            })?;

        let content = localized_docs
            .get(&resolved)
            .or_else(|| english_docs.get(&resolved))
            .cloned()
            .ok_or_else(|| CoastError::state(format!("resolved docs path missing: {resolved}")))?;

        return Ok(DocsResponse {
            locale,
            tree,
            path: Some(resolved),
            content: Some(content),
        });
    }

    Ok(DocsResponse {
        locale,
        tree,
        path: None,
        content: None,
    })
}

/// Handle a docs search request (hybrid keyword + semantic).
pub async fn handle_search_docs(
    req: SearchDocsRequest,
    state: &AppState,
) -> Result<SearchDocsResponse> {
    let locale = resolve_locale(req.language.as_deref(), &state.language());
    let index = load_search_index_with_fallback(&locale)?;
    let limit = req
        .limit
        .unwrap_or(SEARCH_DEFAULT_LIMIT)
        .clamp(1, SEARCH_MAX_LIMIT);

    let results = rank_search(&index, &req.query, limit);

    info!(
        locale = %index.locale,
        query = %req.query,
        count = results.len(),
        "docs search completed"
    );

    Ok(SearchDocsResponse {
        query: req.query,
        locale: index.locale.clone(),
        strategy: "hybrid_keyword_semantic".to_string(),
        results,
    })
}

fn resolve_locale(requested: Option<&str>, daemon_lang: &str) -> String {
    if let Some(lang) = requested {
        if coast_i18n::is_valid_language(lang) {
            return lang.to_string();
        }
    }
    if coast_i18n::is_valid_language(daemon_lang) {
        return daemon_lang.to_string();
    }
    "en".to_string()
}

fn load_docs_for_locale(locale: &str) -> Result<HashMap<String, String>> {
    let mut out = HashMap::new();

    for path in DocsAssets::iter() {
        let p = path.as_ref();
        if !p.ends_with(".md") {
            continue;
        }

        let rel = if locale == "en" {
            if LOCALES.iter().any(|l| p.starts_with(&format!("{l}/"))) {
                continue;
            }
            p.to_string()
        } else {
            let prefix = format!("{locale}/");
            if let Some(stripped) = p.strip_prefix(&prefix) {
                stripped.to_string()
            } else {
                continue;
            }
        };

        let Some(file) = DocsAssets::get(p) else {
            continue;
        };
        let content = std::str::from_utf8(file.data.as_ref())
            .map_err(|e| CoastError::protocol(format!("invalid UTF-8 in docs asset '{p}': {e}")))?
            .to_string();
        out.insert(rel, content);
    }

    Ok(out)
}

fn build_docs_tree(paths: impl IntoIterator<Item = String>) -> Vec<DocsNode> {
    let mut root = DirTree::default();
    for path in paths {
        root.insert_file(&path);
    }
    build_nodes_from_tree("", &root)
}

fn build_nodes_from_tree(prefix: &str, tree: &DirTree) -> Vec<DocsNode> {
    let mut files: Vec<String> = tree.files.iter().cloned().collect();
    files.sort_by(file_name_cmp);

    let mut nodes = Vec::new();

    for file in files {
        let full_path = join_rel(prefix, &file);
        nodes.push(DocsNode {
            name: file,
            path: full_path,
            kind: DocsNodeKind::File,
            children: Vec::new(),
        });
    }

    for (dir_name, dir_node) in &tree.dirs {
        let full_path = join_rel(prefix, dir_name);
        nodes.push(DocsNode {
            name: dir_name.clone(),
            path: full_path.clone(),
            kind: DocsNodeKind::Dir,
            children: build_nodes_from_tree(&full_path, dir_node),
        });
    }

    nodes
}

fn file_name_cmp(a: &String, b: &String) -> std::cmp::Ordering {
    match (a.as_str() == "README.md", b.as_str() == "README.md") {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.cmp(b),
    }
}

fn join_rel(prefix: &str, part: &str) -> String {
    if prefix.is_empty() {
        part.to_string()
    } else {
        format!("{prefix}/{part}")
    }
}

fn resolve_markdown_path(
    requested: &str,
    localized_docs: &HashMap<String, String>,
    english_docs: &HashMap<String, String>,
) -> Option<String> {
    let normalized = requested.trim().trim_matches('/');
    let candidates = if normalized.is_empty() {
        vec!["README.md".to_string()]
    } else if normalized.ends_with(".md") {
        vec![normalized.to_string()]
    } else {
        vec![
            normalized.to_string(),
            format!("{normalized}.md"),
            format!("{normalized}/README.md"),
        ]
    };

    for c in &candidates {
        if localized_docs.contains_key(c) {
            return Some(c.clone());
        }
    }
    for c in &candidates {
        if english_docs.contains_key(c) {
            return Some(c.clone());
        }
    }
    None
}

fn load_search_index_with_fallback(locale: &str) -> Result<SearchIndexData> {
    if let Some(index) = load_search_index(locale)? {
        return Ok(index);
    }

    if locale != "en" {
        if let Some(index) = load_search_index("en")? {
            return Ok(index);
        }
    }

    Err(CoastError::state(
        "search index assets not found; regenerate search-indexes for CLI/daemon",
    ))
}

fn load_search_index(locale: &str) -> Result<Option<SearchIndexData>> {
    let file_name = format!("docs-search-index-{locale}.json");
    let Some(file) = SearchIndexAssets::get(&file_name) else {
        return Ok(None);
    };

    let raw = std::str::from_utf8(file.data.as_ref()).map_err(|e| {
        CoastError::protocol(format!("invalid UTF-8 in search index '{file_name}': {e}"))
    })?;
    let parsed: SearchIndexData = serde_json::from_str(raw).map_err(|e| {
        CoastError::protocol(format!("invalid JSON in search index '{file_name}': {e}"))
    })?;
    Ok(Some(parsed))
}

fn rank_search(index: &SearchIndexData, query: &str, limit: usize) -> Vec<SearchDocsResult> {
    let query_tokens = tokenize(query);
    if query_tokens.is_empty() {
        return Vec::new();
    }

    let mut scores: HashMap<usize, f64> = HashMap::new();

    for token in &query_tokens {
        let Some(postings) = index.inverted_index.get(token) else {
            continue;
        };
        let Some(idf_val) = index.idf.get(token) else {
            continue;
        };

        for entry in postings {
            let Some(section) = index.sections.get(entry.s) else {
                continue;
            };
            let s = bm25_score(
                entry.tf as f64,
                *idf_val,
                section.token_count as f64,
                index.avg_dl,
            );
            *scores.entry(entry.s).or_insert(0.0) += s;
        }
    }

    let mut sorted_by_bm25: Vec<(usize, f64)> = scores.iter().map(|(k, v)| (*k, *v)).collect();
    sorted_by_bm25.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    sorted_by_bm25.truncate(20);

    let mut boosted = scores.clone();
    for (section_id, section_score) in sorted_by_bm25 {
        let neighbors_key = section_id.to_string();
        let Some(neighbors) = index.semantic_neighbors.get(&neighbors_key) else {
            continue;
        };
        for n in neighbors {
            let boost = section_score * n.score * SEMANTIC_BOOST_WEIGHT;
            *boosted.entry(n.s).or_insert(0.0) += boost;
        }
    }

    let mut ranked: Vec<(usize, f64)> = boosted.into_iter().collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(limit);

    ranked
        .into_iter()
        .filter_map(|(section_id, score)| {
            let section = index.sections.get(section_id)?;
            Some(SearchDocsResult {
                path: section.file_path.clone(),
                route: section.route.clone(),
                heading: section.heading.clone(),
                snippet: extract_snippet(&section.content, query),
                score,
            })
        })
        .collect()
}

fn bm25_score(tf: f64, idf: f64, doc_len: f64, avg_dl: f64) -> f64 {
    let avg = if avg_dl > 0.0 { avg_dl } else { 1.0 };
    let num = tf * (BM25_K1 + 1.0);
    let denom = tf + BM25_K1 * (1.0 - BM25_B + BM25_B * (doc_len / avg));
    idf * (num / denom)
}

fn tokenize(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut tokens = Vec::new();

    let mut current = String::new();
    for ch in lower.chars() {
        if ch.is_alphanumeric() {
            current.push(ch);
        } else if !current.is_empty() {
            if current.chars().count() >= 2 && !STOP_WORDS.contains(&current.as_str()) {
                tokens.push(current.clone());
            }
            current.clear();
        }
    }
    if !current.is_empty()
        && current.chars().count() >= 2
        && !STOP_WORDS.contains(&current.as_str())
    {
        tokens.push(current);
    }

    let chars: Vec<char> = lower.chars().collect();
    for pair in chars.windows(2) {
        if let [a, b] = pair {
            if is_cjk(*a) && is_cjk(*b) {
                tokens.push(format!("{a}{b}"));
            }
        }
    }

    tokens
}

fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3000..=0x303F | 0x3040..=0x309F | 0x30A0..=0x30FF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xFF00..=0xFFEF
    )
}

fn extract_snippet(content: &str, query: &str) -> String {
    let lines: Vec<&str> = content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();

    if lines.is_empty() {
        return String::new();
    }

    let query_words: Vec<String> = query
        .to_lowercase()
        .split_whitespace()
        .map(std::string::ToString::to_string)
        .collect();

    let mut best_idx = 0usize;
    let mut best_score = -1i32;
    for (i, line) in lines.iter().enumerate() {
        let ll = line.to_lowercase();
        let mut line_score = 0i32;
        for word in &query_words {
            if !word.is_empty() && ll.contains(word) {
                line_score += 1;
            }
        }
        if line_score > best_score {
            best_score = line_score;
            best_idx = i;
        }
    }

    let mut snippet = lines[best_idx].to_string();
    if snippet.chars().count() > 200 {
        snippet = snippet.chars().take(200).collect::<String>() + "...";
    }
    snippet
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_index() -> SearchIndexData {
        SearchIndexData {
            locale: "en".to_string(),
            sections: vec![
                SearchSection {
                    file_path: "coastfiles/COASTFILE.md".to_string(),
                    heading: "Volume Strategy".to_string(),
                    content: "Use isolated volume strategy for local development.".to_string(),
                    route: "/docs/coastfiles/COASTFILE".to_string(),
                    token_count: 8,
                },
                SearchSection {
                    file_path: "shared/SERVICES.md".to_string(),
                    heading: "Shared Services".to_string(),
                    content: "Databases and services can be shared across instances.".to_string(),
                    route: "/docs/shared/SERVICES".to_string(),
                    token_count: 8,
                },
            ],
            inverted_index: HashMap::from([("volume".to_string(), vec![InvEntry { s: 0, tf: 1 }])]),
            idf: HashMap::from([("volume".to_string(), 2.0)]),
            semantic_neighbors: HashMap::from([(
                "0".to_string(),
                vec![Neighbor { s: 1, score: 1.0 }],
            )]),
            avg_dl: 8.0,
        }
    }

    #[test]
    fn test_build_docs_tree_readme_first() {
        let tree = build_docs_tree(vec![
            "coastfiles/COASTFILE.md".to_string(),
            "README.md".to_string(),
            "GETTING_STARTED.md".to_string(),
            "coastfiles/README.md".to_string(),
        ]);

        assert_eq!(tree[0].name, "README.md");
        assert_eq!(tree[0].kind, DocsNodeKind::File);
        assert_eq!(tree[1].name, "GETTING_STARTED.md");
        assert_eq!(tree[2].name, "coastfiles");
        assert_eq!(tree[2].kind, DocsNodeKind::Dir);
    }

    #[test]
    fn test_resolve_markdown_path_prefers_folder_readme() {
        let localized = HashMap::from([(
            "coastfiles/README.md".to_string(),
            "# Coastfiles".to_string(),
        )]);
        let en = HashMap::new();

        let resolved = resolve_markdown_path("coastfiles", &localized, &en).unwrap();
        assert_eq!(resolved, "coastfiles/README.md");
    }

    #[test]
    fn test_resolve_markdown_path_falls_back_to_en() {
        let localized = HashMap::new();
        let en = HashMap::from([(
            "coastfiles/COASTFILE.md".to_string(),
            "# Coastfile".to_string(),
        )]);

        let resolved = resolve_markdown_path("coastfiles/COASTFILE", &localized, &en).unwrap();
        assert_eq!(resolved, "coastfiles/COASTFILE.md");
    }

    #[test]
    fn test_rank_search_hybrid_boost_includes_semantic_neighbor() {
        let index = fixture_index();
        let results = rank_search(&index, "volume", 10);

        assert!(!results.is_empty());
        assert_eq!(results[0].path, "coastfiles/COASTFILE.md");
        assert!(
            results.iter().any(|r| r.path == "shared/SERVICES.md"),
            "semantic neighbor should be boosted into results"
        );
    }

    // -----------------------------------------------------------------------
    // tokenize() tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_tokenize_ascii_words() {
        let tokens = tokenize("Hello World");
        assert_eq!(tokens, vec!["hello", "world"]);
    }

    #[test]
    fn test_tokenize_filters_stop_words() {
        let tokens = tokenize("the quick and the");
        assert_eq!(tokens, vec!["quick"]);
    }

    #[test]
    fn test_tokenize_rejects_single_char_tokens() {
        let tokens = tokenize("a b cd");
        assert_eq!(tokens, vec!["cd"]);
    }

    #[test]
    fn test_tokenize_empty_input() {
        assert!(tokenize("").is_empty());
        assert!(tokenize("   ").is_empty());
    }

    #[test]
    fn test_tokenize_cjk_bigrams() {
        let tokens = tokenize("海岸");
        assert!(tokens.contains(&"海岸".to_string()));
    }

    #[test]
    fn test_tokenize_mixed_ascii_and_cjk() {
        let tokens = tokenize("coast 海岸 guard");
        assert!(tokens.contains(&"coast".to_string()));
        assert!(tokens.contains(&"guard".to_string()));
        assert!(tokens.contains(&"海岸".to_string()));
    }

    #[test]
    fn test_tokenize_accented_latin() {
        let tokens = tokenize("résumé café");
        assert!(tokens.contains(&"résumé".to_string()));
        assert!(tokens.contains(&"café".to_string()));
    }

    #[test]
    fn test_tokenize_case_insensitive() {
        let tokens = tokenize("Docker COMPOSE Volumes");
        assert_eq!(tokens, vec!["docker", "compose", "volumes"]);
    }

    // -----------------------------------------------------------------------
    // bm25_score() tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_bm25_score_basic() {
        let score = bm25_score(1.0, 2.0, 10.0, 10.0);
        assert!(score > 0.0);
    }

    #[test]
    fn test_bm25_score_zero_avg_dl_guard() {
        let score = bm25_score(1.0, 2.0, 10.0, 0.0);
        assert!(score > 0.0, "zero avgDl should not cause division by zero");
    }

    #[test]
    fn test_bm25_score_high_tf_saturates() {
        let low_tf = bm25_score(1.0, 2.0, 10.0, 10.0);
        let high_tf = bm25_score(100.0, 2.0, 10.0, 10.0);
        assert!(high_tf > low_tf, "higher TF should increase score");
        assert!(
            high_tf < 100.0 * low_tf,
            "BM25 saturates so 100x TF should not yield 100x score"
        );
    }

    // -----------------------------------------------------------------------
    // rank_search() tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_rank_search_no_matching_tokens() {
        let index = fixture_index();
        let results = rank_search(&index, "xyznonexistent", 10);
        assert!(results.is_empty());
    }

    #[test]
    fn test_rank_search_empty_query() {
        let index = fixture_index();
        let results = rank_search(&index, "", 10);
        assert!(results.is_empty());
    }

    #[test]
    fn test_rank_search_limit_respected() {
        let index = fixture_index();
        let results = rank_search(&index, "volume", 1);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_rank_search_scores_descending() {
        let index = fixture_index();
        let results = rank_search(&index, "volume", 10);
        for window in results.windows(2) {
            assert!(
                window[0].score >= window[1].score,
                "results should be sorted by descending score"
            );
        }
    }

    #[test]
    fn test_rank_search_multi_token_query() {
        let mut index = fixture_index();
        index
            .inverted_index
            .insert("shared".to_string(), vec![InvEntry { s: 1, tf: 1 }]);
        index.idf.insert("shared".to_string(), 2.0);

        let results = rank_search(&index, "volume shared", 10);
        assert!(results.len() >= 2);
    }

    // -----------------------------------------------------------------------
    // extract_snippet() tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_extract_snippet_picks_matching_line() {
        let content = "# Heading\nFirst line about ports.\nSecond line about volumes.";
        let snippet = extract_snippet(content, "volumes");
        assert!(snippet.contains("volumes"));
    }

    #[test]
    fn test_extract_snippet_truncates_at_200_chars() {
        let long_line = "x".repeat(300);
        let content = format!("# Heading\n{long_line}");
        let snippet = extract_snippet(&content, "x");
        assert!(snippet.len() <= 204); // 200 + "..."
        assert!(snippet.ends_with("..."));
    }

    #[test]
    fn test_extract_snippet_empty_content() {
        let snippet = extract_snippet("", "anything");
        assert!(snippet.is_empty());
    }

    #[test]
    fn test_extract_snippet_heading_only_content() {
        let snippet = extract_snippet("# Just a Heading", "heading");
        assert!(snippet.is_empty());
    }

    // -----------------------------------------------------------------------
    // resolve_locale() tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_resolve_locale_explicit() {
        let result = resolve_locale(Some("es"), "en");
        assert_eq!(result, "es");
    }

    #[test]
    fn test_resolve_locale_daemon_fallback() {
        let result = resolve_locale(None, "ja");
        assert_eq!(result, "ja");
    }

    #[test]
    fn test_resolve_locale_invalid_falls_back_to_en() {
        let result = resolve_locale(Some("xx"), "yy");
        assert_eq!(result, "en");
    }

    #[test]
    fn test_resolve_locale_none_and_invalid_daemon() {
        let result = resolve_locale(None, "invalid");
        assert_eq!(result, "en");
    }
}
