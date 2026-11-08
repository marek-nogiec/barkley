//! Barkley plugin: bookmarks. `g kl` opens the URL saved under the alias `kl`.
//!
//! Shares the `g` keyword with Web Search, so both run: a matching bookmark ranks above
//! the "Search Google for …" row, and a query that isn't an alias just searches.
//! Bookmarks are read from a TOML file on every query, so edits apply immediately.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use barkley_plugin::Item;
use serde::Deserialize;

/// Written when the file doesn't exist yet.
const TEMPLATE: &str = r#"# Barkley bookmarks: type `g <alias>` and press Return to open the URL.
# Saved changes apply on the next query, no restart needed.
#
# alias = "url"
# alias = { url = "url", title = "Shown instead of the URL" }
#
# kl = "https://example.com/"
# panel = { url = "https://example.com/panel/app", title = "Example panel" }
"#;

// Above Web Search's row (score 1), so Return opens the bookmark.
const EXACT: u32 = 30;
const PREFIX: u32 = 20;
const CONTAINS: u32 = 10;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Entry {
    Url(String),
    Full { url: String, title: Option<String> },
}

#[derive(Debug, PartialEq, Eq)]
struct Bookmark {
    alias: String,
    url: String,
    title: Option<String>,
}

impl Bookmark {
    fn item(&self, score: u32) -> Item {
        let title = self.title.clone().unwrap_or_else(|| self.url.clone());
        Item::new(title).subtitle(&self.url).tag(&self.alias).symbol("bookmark").score(score).open_url(&self.url)
    }
}

/// Sorted by alias.
fn parse(src: &str) -> Result<Vec<Bookmark>, String> {
    let map: BTreeMap<String, Entry> = toml::from_str(src).map_err(|e| e.message().to_string())?;
    Ok(map
        .into_iter()
        .map(|(alias, entry)| match entry {
            Entry::Url(url) => Bookmark { alias, url, title: None },
            Entry::Full { url, title } => Bookmark { alias, url, title },
        })
        .collect())
}

/// `$BARKLEY_BOOKMARKS_FILE` (with `~/` expanded), else Barkley's Application Support folder.
fn file_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    match std::env::var("BARKLEY_BOOKMARKS_FILE") {
        Ok(p) if !p.is_empty() => match p.strip_prefix("~/") {
            Some(rest) => Path::new(&home).join(rest),
            None => PathBuf::from(p),
        },
        _ => Path::new(&home).join("Library/Application Support/Barkley/bookmarks.toml"),
    }
}

/// Reads the file, creating it from [`TEMPLATE`] if it's missing.
fn load(path: &Path) -> Result<Vec<Bookmark>, String> {
    match std::fs::read_to_string(path) {
        Ok(src) => parse(&src),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(path, TEMPLATE);
            Ok(Vec::new())
        }
        Err(e) => Err(e.to_string()),
    }
}

fn edit_item(path: &Path) -> Item {
    let p = path.display().to_string();
    Item::new("Edit bookmarks").subtitle(&p).symbol("square.and.pencil").verb("Edit").run("/usr/bin/open", ["-t", &p])
}

fn items(query: &str, loaded: Result<Vec<Bookmark>, String>, path: &Path) -> Vec<Item> {
    let bookmarks = match loaded {
        Ok(b) => b,
        Err(msg) => {
            let file = path.file_name().map_or_else(String::new, |f| f.to_string_lossy().into_owned());
            return vec![
                Item::new(format!("Can't read {file}"))
                    .subtitle(msg)
                    .score(EXACT)
                    .symbol("exclamationmark.triangle")
                    .verb("Edit")
                    .run("/usr/bin/open", ["-t", &path.display().to_string()]),
            ];
        }
    };

    let q = query.trim().to_lowercase();
    // Bare `g`: the edit row first (so it survives the 9-row cap), then every bookmark.
    if q.is_empty() {
        let mut out = vec![edit_item(path).score(EXACT)];
        out.extend(bookmarks.iter().map(|b| b.item(PREFIX)));
        return out;
    }

    bookmarks
        .iter()
        .filter_map(|b| {
            let alias = b.alias.to_lowercase();
            let score = if alias == q {
                EXACT
            } else if alias.starts_with(&q) {
                PREFIX
            } else if b.title.as_deref().unwrap_or("").to_lowercase().contains(&q) || b.url.to_lowercase().contains(&q)
            {
                CONTAINS
            } else {
                return None;
            };
            Some(b.item(score))
        })
        .collect()
}

fn main() {
    let path = file_path();
    barkley_plugin::run(|query| items(query, load(&path), &path));
}

#[cfg(test)]
mod tests {
    use super::*;
    use barkley_plugin::Action;

    const SRC: &str = r#"
kl = "https://kleszcz.com.pl/"
panel = { url = "https://kleszcz.com.pl/panel/app", title = "Kleszcz panel" }
"#;

    fn run(q: &str) -> Vec<Item> {
        items(q, parse(SRC), Path::new("/x/bookmarks.toml"))
    }

    fn url(it: &Item) -> &str {
        match &it.action {
            Some(Action::OpenUrl { url }) => url,
            other => panic!("not open_url: {other:?}"),
        }
    }

    #[test]
    fn parses_both_forms() {
        let b = parse(SRC).unwrap();
        assert_eq!(b[0], Bookmark { alias: "kl".into(), url: "https://kleszcz.com.pl/".into(), title: None });
        assert_eq!(b[1].title.as_deref(), Some("Kleszcz panel"));
    }

    #[test]
    fn exact_alias_opens_url() {
        let out = run("panel");
        assert_eq!(out.len(), 1);
        assert_eq!(url(&out[0]), "https://kleszcz.com.pl/panel/app");
        assert_eq!(out[0].title, "Kleszcz panel");
        assert_eq!(out[0].score, Some(EXACT));
    }

    #[test]
    fn exact_beats_prefix_and_contains() {
        let out = run("KL");
        let kl = out.iter().find(|i| url(i) == "https://kleszcz.com.pl/").unwrap();
        assert_eq!(kl.score, Some(EXACT));
        // "panel" matches by its URL/title containing "kl".
        assert!(out.iter().any(|i| i.score == Some(CONTAINS)));
    }

    #[test]
    fn prefix_matches() {
        assert_eq!(run("pa")[0].score, Some(PREFIX));
    }

    #[test]
    fn unknown_alias_is_empty() {
        assert!(run("rust lang").is_empty());
    }

    #[test]
    fn bare_keyword_lists_edit_row_then_all() {
        let out = run("");
        assert_eq!(out[0].title, "Edit bookmarks");
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn template_parses_to_nothing() {
        assert!(parse(TEMPLATE).unwrap().is_empty());
    }

    #[test]
    fn bad_file_shows_error_row() {
        let out = items("kl", parse("kl = 3"), Path::new("/x/bookmarks.toml"));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, "Can't read bookmarks.toml");
    }
}
