/// A parsed user query.
///
/// `keyword` is the first whitespace-delimited word when the query contains a
/// space after it (`"g rust"` → keyword `g`, args `rust`). A single word with
/// no trailing space has no keyword: the user may still be typing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub raw: String,
    pub keyword: Option<String>,
    pub args: String,
}

impl Query {
    pub fn parse(raw: &str) -> Self {
        let trimmed = raw.trim_start();
        let (keyword, args) = match trimmed.split_once(' ') {
            Some((kw, rest)) if !kw.is_empty() => (Some(kw.to_string()), rest.trim().to_string()),
            _ => (None, trimmed.trim().to_string()),
        };
        Self { raw: raw.to_string(), keyword, args }
    }

    /// The whole query, trimmed, regardless of keyword split.
    pub fn text(&self) -> &str {
        self.raw.trim()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    OpenUrl(String),
    Copy(String),
    Run { cmd: String, args: Vec<String> },
    Nothing,
}

/// Row icon. Kept UI-agnostic: the app turns it into an image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Icon {
    /// SF Symbol name, e.g. `"headphones"`.
    Symbol(String),
    /// Absolute path to an image file.
    File(std::path::PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub title: String,
    pub subtitle: String,
    pub score: u32,
    pub action: Action,
    pub icon: Option<Icon>,
    /// Overrides the default action label ("Open", "Copy", "Run").
    pub verb: Option<String>,
    /// Short status at the row's right edge, e.g. "Current".
    pub tag: Option<String>,
    /// Rendered as an error row (e.g. a script plugin failed).
    pub is_error: bool,
}

impl Item {
    pub fn new(title: impl Into<String>, subtitle: impl Into<String>, action: Action) -> Self {
        Self {
            title: title.into(),
            subtitle: subtitle.into(),
            score: 0,
            action,
            icon: None,
            verb: None,
            tag: None,
            is_error: false,
        }
    }

    pub fn with_score(mut self, score: u32) -> Self {
        self.score = score;
        self
    }

    pub fn with_symbol(mut self, name: &str) -> Self {
        self.icon = Some(Icon::Symbol(name.into()));
        self
    }

    pub fn error(title: impl Into<String>, subtitle: impl Into<String>) -> Self {
        Self {
            is_error: true,
            ..Self::new(title, subtitle, Action::Nothing).with_symbol("exclamationmark.triangle.fill")
        }
    }
}

pub trait Plugin: Send + Sync {
    fn id(&self) -> &str;
    /// `Some("g")` = only consulted when the query starts with `"g "`.
    fn keyword(&self) -> Option<&str>;
    /// Consulted only when no other plugin produced results.
    fn is_fallback(&self) -> bool {
        false
    }
    fn query(&self, q: &Query) -> Vec<Item>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_keyword_and_args() {
        let q = Query::parse("g rust lang");
        assert_eq!(q.keyword.as_deref(), Some("g"));
        assert_eq!(q.args, "rust lang");
    }

    #[test]
    fn single_word_has_no_keyword() {
        let q = Query::parse("rust");
        assert_eq!(q.keyword, None);
        assert_eq!(q.args, "rust");
    }

    #[test]
    fn keyword_with_trailing_space_and_empty_args() {
        let q = Query::parse("g ");
        assert_eq!(q.keyword.as_deref(), Some("g"));
        assert_eq!(q.args, "");
    }

    #[test]
    fn leading_whitespace_is_ignored() {
        let q = Query::parse("   echo  hi  ");
        assert_eq!(q.keyword.as_deref(), Some("echo"));
        assert_eq!(q.args, "hi");
        assert_eq!(q.text(), "echo  hi");
    }

    #[test]
    fn empty_query() {
        let q = Query::parse("");
        assert_eq!(q.keyword, None);
        assert_eq!(q.args, "");
    }
}
