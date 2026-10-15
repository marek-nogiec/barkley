use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Utf32Str};

/// Fuzzy matcher (nucleo). Not `Sync`: keep one per thread.
pub struct Matcher {
    inner: nucleo_matcher::Matcher,
    buf: Vec<char>,
}

impl Default for Matcher {
    fn default() -> Self {
        Self { inner: nucleo_matcher::Matcher::new(Config::DEFAULT), buf: Vec::new() }
    }
}

impl Matcher {
    /// Score `haystack` against `needle`; `None` if it doesn't match.
    /// An empty needle matches everything with score 0.
    pub fn score(&mut self, needle: &str, haystack: &str) -> Option<u32> {
        if needle.trim().is_empty() {
            return Some(0);
        }
        let pattern = Pattern::new(needle, CaseMatching::Smart, Normalization::Smart, AtomKind::Fuzzy);
        pattern.score(Utf32Str::new(haystack, &mut self.buf), &mut self.inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_subsequence() {
        let mut m = Matcher::default();
        assert!(m.score("frf", "Firefox").is_some());
        assert!(m.score("xyz", "Firefox").is_none());
    }

    #[test]
    fn prefix_beats_scattered() {
        let mut m = Matcher::default();
        let prefix = m.score("saf", "Safari").unwrap();
        let scattered = m.score("saf", "System Analytics Folder").unwrap();
        assert!(prefix > scattered, "{prefix} <= {scattered}");
    }

    #[test]
    fn empty_needle_matches() {
        assert_eq!(Matcher::default().score("", "anything"), Some(0));
    }
}
