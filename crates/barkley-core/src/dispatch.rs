use std::cmp::Reverse;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::matcher::Matcher;
use crate::plugin::{Item, Plugin, Query};

pub const MAX_RESULTS: usize = 9;

/// Routes a query to plugins, runs them in parallel, merges and ranks results.
#[derive(Default)]
pub struct Dispatcher {
    plugins: Vec<Arc<dyn Plugin>>,
    generation: AtomicU64,
}

impl Dispatcher {
    pub fn new(plugins: Vec<Arc<dyn Plugin>>) -> Self {
        Self { plugins, generation: AtomicU64::new(0) }
    }

    pub fn plugins(&self) -> &[Arc<dyn Plugin>] {
        &self.plugins
    }

    /// Start a new query generation. Results from older generations are stale.
    pub fn next_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn is_current(&self, generation: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == generation
    }

    pub fn search(&self, raw: &str) -> Vec<Item> {
        let q = Query::parse(raw);
        if q.text().is_empty() {
            return Vec::new();
        }

        // A bare keyword ("audio", no trailing space) already selects its plugin.
        let q = match &q.keyword {
            None if self.plugins.iter().any(|p| p.keyword() == Some(q.args.as_str())) => {
                Query { keyword: Some(q.args.clone()), args: String::new(), ..q }
            }
            _ => q,
        };

        let keyworded: Vec<_> = match q.keyword.as_deref() {
            Some(kw) => self.plugins.iter().filter(|p| p.keyword() == Some(kw)).cloned().collect(),
            None => Vec::new(),
        };

        let (mut items, needle) = if !keyworded.is_empty() {
            (run_parallel(&keyworded, &q), q.args.as_str())
        } else {
            let general: Vec<_> =
                self.plugins.iter().filter(|p| p.keyword().is_none() && !p.is_fallback()).cloned().collect();
            let mut items = run_parallel(&general, &q);
            if items.is_empty() {
                let fallbacks: Vec<_> = self.plugins.iter().filter(|p| p.is_fallback()).cloned().collect();
                items = run_parallel(&fallbacks, &q);
            }
            (items, q.text())
        };

        rank(&mut items, needle);
        items.truncate(MAX_RESULTS);
        items
    }
}

fn run_parallel(plugins: &[Arc<dyn Plugin>], q: &Query) -> Vec<Item> {
    match plugins {
        [] => Vec::new(),
        [only] => only.query(q),
        many => std::thread::scope(|s| {
            let handles: Vec<_> = many.iter().map(|p| s.spawn(move || p.query(q))).collect();
            handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
        }),
    }
}

/// Sort by plugin score desc; ties broken by fuzzy match of the title against
/// the query. Stable, so plugin order survives full ties.
fn rank(items: &mut Vec<Item>, needle: &str) {
    let mut m = Matcher::default();
    let mut keyed: Vec<_> =
        items.drain(..).map(|it| ((it.score, m.score(needle, &it.title).unwrap_or(0)), it)).collect();
    keyed.sort_by_key(|(k, _)| Reverse(*k));
    items.extend(keyed.into_iter().map(|(_, it)| it));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::Action;

    struct Fixed {
        id: &'static str,
        keyword: Option<&'static str>,
        fallback: bool,
        items: Vec<(&'static str, u32)>,
    }

    impl Plugin for Fixed {
        fn id(&self) -> &str {
            self.id
        }
        fn keyword(&self) -> Option<&str> {
            self.keyword
        }
        fn is_fallback(&self) -> bool {
            self.fallback
        }
        fn query(&self, _q: &Query) -> Vec<Item> {
            self.items.iter().map(|(t, s)| Item::new(*t, self.id, Action::Nothing).with_score(*s)).collect()
        }
    }

    fn p(
        id: &'static str,
        keyword: Option<&'static str>,
        fallback: bool,
        items: Vec<(&'static str, u32)>,
    ) -> Arc<dyn Plugin> {
        Arc::new(Fixed { id, keyword, fallback, items })
    }

    fn titles(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.title.as_str()).collect()
    }

    #[test]
    fn keyword_routes_to_single_plugin() {
        let d = Dispatcher::new(vec![
            p("kw", Some("g"), false, vec![("kw-item", 1)]),
            p("general", None, false, vec![("general-item", 100)]),
        ]);
        assert_eq!(titles(&d.search("g foo")), ["kw-item"]);
    }

    #[test]
    fn bare_keyword_routes_to_plugin() {
        let d = Dispatcher::new(vec![
            p("audio", Some("audio"), false, vec![("device", 1)]),
            p("general", None, false, vec![("general-item", 1)]),
        ]);
        assert_eq!(titles(&d.search("audio")), ["device"]);
        assert_eq!(titles(&d.search("audi")), ["general-item"]);
    }

    #[test]
    fn unknown_keyword_goes_to_general_plugins() {
        let d = Dispatcher::new(vec![
            p("kw", Some("g"), false, vec![("kw-item", 1)]),
            p("general", None, false, vec![("general-item", 1)]),
        ]);
        assert_eq!(titles(&d.search("x foo")), ["general-item"]);
    }

    #[test]
    fn fallback_only_when_nothing_else_matched() {
        let d = Dispatcher::new(vec![p("fb", Some("g"), true, vec![("fallback", 1)]), p("empty", None, false, vec![])]);
        assert_eq!(titles(&d.search("anything")), ["fallback"]);

        let d = Dispatcher::new(vec![
            p("fb", Some("g"), true, vec![("fallback", 1)]),
            p("general", None, false, vec![("hit", 1)]),
        ]);
        assert_eq!(titles(&d.search("anything")), ["hit"]);
    }

    #[test]
    fn merges_and_sorts_by_score_desc() {
        let d = Dispatcher::new(vec![
            p("a", None, false, vec![("low", 10), ("high", 90)]),
            p("b", None, false, vec![("mid", 50)]),
        ]);
        assert_eq!(titles(&d.search("q")), ["high", "mid", "low"]);
    }

    #[test]
    fn ties_broken_by_fuzzy_match() {
        let d = Dispatcher::new(vec![p("a", None, false, vec![("Zebra", 5), ("Safari", 5)])]);
        assert_eq!(titles(&d.search("saf")), ["Safari", "Zebra"]);
    }

    #[test]
    fn caps_results() {
        let many: Vec<_> = (0..20).map(|i| ("x", i)).collect();
        let d = Dispatcher::new(vec![p("a", None, false, many)]);
        assert_eq!(d.search("q").len(), MAX_RESULTS);
    }

    #[test]
    fn empty_query_returns_nothing() {
        let d = Dispatcher::new(vec![p("fb", None, true, vec![("fallback", 1)])]);
        assert!(d.search("   ").is_empty());
    }

    #[test]
    fn generations_invalidate_older_queries() {
        let d = Dispatcher::default();
        let g1 = d.next_generation();
        assert!(d.is_current(g1));
        let g2 = d.next_generation();
        assert!(!d.is_current(g1));
        assert!(d.is_current(g2));
    }
}
