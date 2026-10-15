//! Runs the real binary the way Barkley does and parses its stdout.

use std::process::Command;

use barkley_plugin::{Action, Output};

fn run(query: &str) -> Output {
    let out = Command::new(env!("CARGO_BIN_EXE_barkley-units")).arg(query).output().unwrap();
    assert!(out.status.success());
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn converts_via_protocol() {
    let out = run("10 km to mi");
    assert_eq!(out.items[0].title, "6.21 mi");
    assert_eq!(out.items[0].action, Some(Action::Copy { text: "6.21".into() }));
}

#[test]
fn non_conversion_prints_empty_list() {
    assert!(run("rust lang").items.is_empty());
}
