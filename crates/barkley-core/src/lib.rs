//! Barkley core: plugin model, query dispatch, settings. No AppKit here.

pub mod dispatch;
pub mod matcher;
pub mod plugin;
pub mod script_plugin;
pub mod settings;

use std::path::PathBuf;
use std::sync::Arc;

pub use dispatch::Dispatcher;
pub use plugin::{Action, Icon, Item, Plugin, Query};
pub use settings::Settings;

/// Every plugin in `bundled` (in order), then the user's plugins folder; a
/// later folder with the same name wins. Returns the dispatcher and any load errors.
pub fn build_dispatcher(settings: &Settings, bundled: &[PathBuf]) -> (Dispatcher, Vec<String>) {
    let mut dirs = bundled.to_vec();
    dirs.push(settings.plugins_dir());
    let (plugins, errors) = script_plugin::load_all(&dirs, &settings.plugin);
    (Dispatcher::new(plugins.into_iter().map(|p| Arc::new(p) as Arc<dyn Plugin>).collect()), errors)
}
