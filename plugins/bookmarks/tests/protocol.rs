//! Runs the real binary the way Barkley does, against a temporary bookmarks file.

use std::path::PathBuf;
use std::process::Command;

use barkley_plugin::{Action, Output};

fn temp_file(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("barkley-bookmarks-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("bookmarks.toml")
}

fn run(file: &PathBuf, query: &str) -> Output {
    let out = Command::new(env!("CARGO_BIN_EXE_barkley-bookmarks"))
        .arg(query)
        .env("BARKLEY_BOOKMARKS_FILE", file)
        .output()
        .unwrap();
    assert!(out.status.success());
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn opens_alias_via_protocol() {
    let file = temp_file("open");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "kl = \"https://kleszcz.com.pl/\"\n").unwrap();
    let out = run(&file, "kl");
    assert_eq!(out.items[0].action, Some(Action::OpenUrl { url: "https://kleszcz.com.pl/".into() }));
}

#[test]
fn creates_missing_file_from_template() {
    let file = temp_file("create");
    assert!(run(&file, "kl").items.is_empty());
    assert!(std::fs::read_to_string(&file).unwrap().contains("g <alias>"));
}
