//! Barkley plugin protocol, plus helpers for writing plugins in Rust.
//!
//! A plugin is a folder with a `manifest.toml` and an executable. Barkley runs
//! the executable with the query as `argv[1]` (and `$BARKLEY_QUERY`) and reads
//! `{"items": [...]}` from stdout. These types are that JSON, used by both the
//! app (to parse) and Rust plugins (to print).
//!
//! With `persistent = true` in the manifest, Barkley starts the plugin once
//! (with `BARKLEY_PERSISTENT=1`) and sends one [`Request`] per line on stdin;
//! the plugin answers each with one [`Response`] line. [`run`] handles both
//! modes, so plugin code doesn't change.
//!
//! ```no_run
//! use barkley_plugin::{Item, run};
//!
//! fn main() {
//!     run(|query| vec![Item::new(format!("You typed {query}")).copy(query)]);
//! }
//! ```

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Output {
    pub items: Vec<Item>,
}

/// Persistent mode: one line on the plugin's stdin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub query: String,
}

/// Persistent mode: one line on the plugin's stdout, answering [`Request`] `id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub subtitle: String,
    /// Higher ranks first. Without it, items keep the order they were printed in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<Action>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
    /// Label shown next to ↩ on the selected row, e.g. `"Connect"`. Defaults by action type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verb: Option<String>,
    /// Short status shown at the row's right edge, e.g. `"Current"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    OpenUrl { url: String },
    Copy { text: String },
    Run {
        cmd: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Icon {
    /// SF Symbol name, e.g. `"headphones"`.
    Symbol { symbol: String },
    /// Image path, relative to the plugin folder.
    Path { path: String },
}

impl Item {
    pub fn new(title: impl Into<String>) -> Self {
        Self { title: title.into(), ..Self::default() }
    }

    pub fn tag(mut self, t: impl Into<String>) -> Self {
        self.tag = Some(t.into());
        self
    }

    pub fn verb(mut self, v: impl Into<String>) -> Self {
        self.verb = Some(v.into());
        self
    }

    pub fn subtitle(mut self, s: impl Into<String>) -> Self {
        self.subtitle = s.into();
        self
    }

    pub fn score(mut self, score: u32) -> Self {
        self.score = Some(score);
        self
    }

    pub fn symbol(mut self, name: impl Into<String>) -> Self {
        self.icon = Some(Icon::Symbol { symbol: name.into() });
        self
    }

    pub fn open_url(mut self, url: impl Into<String>) -> Self {
        self.action = Some(Action::OpenUrl { url: url.into() });
        self
    }

    pub fn copy(mut self, text: impl Into<String>) -> Self {
        self.action = Some(Action::Copy { text: text.into() });
        self
    }

    pub fn run(mut self, cmd: impl Into<String>, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.action = Some(Action::Run { cmd: cmd.into(), args: args.into_iter().map(Into::into).collect() });
        self
    }
}

/// The query Barkley passed: `argv[1]`, falling back to `$BARKLEY_QUERY`.
pub fn query() -> String {
    std::env::args().nth(1).or_else(|| std::env::var("BARKLEY_QUERY").ok()).unwrap_or_default()
}

/// Print items as the protocol JSON.
pub fn print(items: Vec<Item>) {
    let out = serde_json::to_string(&Output { items }).expect("items serialize");
    println!("{out}");
}

/// Read the query, call `f`, print its items. In persistent mode, keep
/// answering requests from stdin until it closes.
pub fn run(mut f: impl FnMut(&str) -> Vec<Item>) {
    if std::env::var_os("BARKLEY_PERSISTENT").is_some_and(|v| v == "1") {
        serve(std::io::stdin().lock(), std::io::stdout().lock(), f);
    } else {
        print(f(&query()));
    }
}

/// The persistent-mode loop over any line reader/writer (testable).
pub fn serve(input: impl std::io::BufRead, mut output: impl std::io::Write, mut f: impl FnMut(&str) -> Vec<Item>) {
    for line in input.lines() {
        let Ok(line) = line else { break };
        let Ok(req) = serde_json::from_str::<Request>(&line) else { continue };
        let resp = Response { id: req.id, items: f(&req.query) };
        let json = serde_json::to_string(&resp).expect("items serialize");
        if writeln!(output, "{json}").and_then(|()| output.flush()).is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_shape() {
        let items = vec![
            Item::new("a").open_url("https://x").symbol("star"),
            Item::new("b").subtitle("s").run("say", ["hi"]),
            Item::new("c").copy("t").score(5),
            Item { icon: Some(Icon::Path { path: "i.png".into() }), ..Item::new("d") },
        ];
        let json = serde_json::to_string(&Output { items: items.clone() }).unwrap();
        let back: Output = serde_json::from_str(&json).unwrap();
        assert_eq!(back.items, items);
    }

    #[test]
    fn minimal_json_parses() {
        let out: Output = serde_json::from_str(r#"{"items":[{"title":"x"}]}"#).unwrap();
        assert_eq!(out.items, [Item::new("x")]);
    }

    #[test]
    fn serve_answers_each_request_by_id() {
        let input = "{\"id\":7,\"query\":\"a\"}\nnot json\n{\"id\":8,\"query\":\"b\"}\n";
        let mut out = Vec::new();
        serve(input.as_bytes(), &mut out, |q| vec![Item::new(q.to_uppercase())]);
        let lines: Vec<Response> =
            String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines, [
            Response { id: 7, items: vec![Item::new("A")] },
            Response { id: 8, items: vec![Item::new("B")] },
        ]);
    }

    #[test]
    fn skips_empty_fields_when_printing() {
        let json = serde_json::to_string(&Item::new("x")).unwrap();
        assert_eq!(json, r#"{"title":"x"}"#);
    }
}
