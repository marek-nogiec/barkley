//! Keeps `plugins/example-echo` working against the real protocol.

use std::path::Path;

use barkley_core::script_plugin::ScriptPlugin;
use barkley_core::{Action, Icon, Plugin, Query};

fn echo() -> ScriptPlugin {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/example-echo");
    ScriptPlugin::load(&dir).expect("example plugin loads")
}

#[test]
fn echoes_with_every_action_type() {
    let items = echo().query(&Query::parse(r#"echo hi "there" \o/"#));
    assert!(!items.iter().any(|i| i.is_error), "{items:?}");
    assert_eq!(items[0].title, r#"hi "there" \o/"#);
    assert_eq!(items[0].action, Action::Copy(r#"hi "there" \o/"#.into()));
    assert_eq!(items[0].icon, Some(Icon::Symbol("doc.on.doc".into())));
    assert!(matches!(&items[1].action, Action::Run { cmd, args } if cmd == "/usr/bin/say" && args.len() == 1));
    assert!(matches!(&items[2].action, Action::OpenUrl(u) if u.starts_with("dict://hi%20%22there%22")));
}

#[test]
fn empty_query_prompts() {
    let items = echo().query(&Query::parse("echo "));
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].action, Action::Nothing);
    assert!(!items[0].is_error);
}
