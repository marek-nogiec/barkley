use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const DEFAULT_CONFIG: &str = r#"# Barkley config. Changes apply on restart (or "Reload plugins" for the plugins section).

[hotkey]
# Double-tap Command to toggle the panel. Needs Accessibility permission.
double_cmd = true
# Chord fallback, works without any permission. Set to "" to disable.
chord = "alt+space"

[plugins]
# Your own plugins. A folder here with the same name as a bundled plugin replaces it.
dir = "~/Library/Application Support/Barkley/plugins"

# Per-plugin overrides, keyed by plugin folder name (bundled or yours):
#
# [plugin.web-search]
# keyword = "s"                 # "" removes the keyword
# env = { BARKLEY_SEARCH_URL = "https://duckduckgo.com/?q={query}", BARKLEY_SEARCH_NAME = "DuckDuckGo" }
#
# [plugin.audio]
# enabled = false
"#;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub hotkey: HotkeySettings,
    pub plugins: PluginSettings,
    /// `[plugin.<folder-name>]` sections.
    pub plugin: BTreeMap<String, PluginOverride>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeySettings {
    pub double_cmd: bool,
    pub chord: String,
}

/// Overrides for one plugin's manifest.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginOverride {
    pub keyword: Option<String>,
    pub enabled: Option<bool>,
    /// Merged over the manifest's `[env]`.
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginSettings {
    pub dir: String,
}

impl Default for HotkeySettings {
    fn default() -> Self {
        Self { double_cmd: true, chord: "alt+space".into() }
    }
}

impl Default for PluginSettings {
    fn default() -> Self {
        Self { dir: "~/Library/Application Support/Barkley/plugins".into() }
    }
}

impl Settings {
    pub fn parse(src: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(src)
    }

    /// Load from `path`, writing the default config there first if it doesn't exist.
    /// A malformed file yields defaults plus the error message, so the app still starts.
    pub fn load_or_create(path: &Path) -> (Self, Option<String>) {
        match std::fs::read_to_string(path) {
            Ok(src) => match Self::parse(&src) {
                Ok(s) => (s, None),
                Err(e) => (Self::default(), Some(format!("{}: {e}", path.display()))),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let err = path
                    .parent()
                    .map_or(Ok(()), std::fs::create_dir_all)
                    .and_then(|()| std::fs::write(path, DEFAULT_CONFIG))
                    .err()
                    .map(|e| format!("{}: {e}", path.display()));
                (Self::default(), err)
            }
            Err(e) => (Self::default(), Some(format!("{}: {e}", path.display()))),
        }
    }

    pub fn plugins_dir(&self) -> PathBuf {
        expand_tilde(&self.plugins.dir)
    }
}

/// `~/Library/Application Support/Barkley`
pub fn app_support_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| expand_tilde("~/Library/Application Support")).join("Barkley")
}

pub fn config_path() -> PathBuf {
    app_support_dir().join("config.toml")
}

pub fn expand_tilde(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => dirs::home_dir().map_or_else(|| PathBuf::from(p), |h| h.join(rest)),
        None if p == "~" => dirs::home_dir().unwrap_or_else(|| PathBuf::from(p)),
        None => PathBuf::from(p),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_text_matches_default_struct() {
        assert_eq!(Settings::parse(DEFAULT_CONFIG).unwrap(), Settings::default());
    }

    #[test]
    fn partial_config_fills_defaults() {
        let s = Settings::parse("[hotkey]\ndouble_cmd = false\n").unwrap();
        assert!(!s.hotkey.double_cmd);
        assert_eq!(s.hotkey.chord, "alt+space");
        assert!(s.plugin.is_empty());
    }

    #[test]
    fn parses_plugin_overrides_and_ignores_legacy_search() {
        let s = Settings::parse(
            "[search]\nengine = \"x\"\n[plugin.web-search]\nkeyword = \"s\"\nenv = { A = \"b\" }\n[plugin.audio]\nenabled = false\n",
        )
        .unwrap();
        assert_eq!(s.plugin["web-search"].keyword.as_deref(), Some("s"));
        assert_eq!(s.plugin["web-search"].env["A"], "b");
        assert_eq!(s.plugin["audio"].enabled, Some(false));
    }

    #[test]
    fn expands_tilde() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand_tilde("~/x/y"), home.join("x/y"));
        assert_eq!(expand_tilde("/abs"), PathBuf::from("/abs"));
    }

    #[test]
    fn load_or_create_writes_defaults_then_reads_them() {
        let dir = std::env::temp_dir().join(format!("barkley-settings-{}", std::process::id()));
        let path = dir.join("nested/config.toml");
        let _ = std::fs::remove_dir_all(&dir);

        let (s, err) = Settings::load_or_create(&path);
        assert_eq!(err, None);
        assert_eq!(s, Settings::default());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), DEFAULT_CONFIG);

        std::fs::write(&path, "not = [valid").unwrap();
        let (s, err) = Settings::load_or_create(&path);
        assert_eq!(s, Settings::default());
        assert!(err.is_some());

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
