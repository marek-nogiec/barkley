//! Plugins: a folder with `manifest.toml` and an executable speaking the
//! `barkley-plugin` JSON protocol (Alfred script-filter style). Every plugin,
//! bundled or user-installed, goes through here.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use barkley_plugin as proto;
use serde::Deserialize;

use crate::plugin::{Action, Icon, Item, Plugin, Query};
use crate::settings::PluginOverride;

const DEFAULT_TIMEOUT_MS: u64 = 1500;
const MAX_OUTPUT: u64 = 1 << 20;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Manifest {
    pub name: String,
    /// Without a keyword the plugin sees every keyword-less query.
    pub keyword: Option<String>,
    /// Path to the executable, relative to the plugin folder.
    pub run: String,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    /// Also consulted when no other plugin produced results.
    #[serde(default)]
    pub fallback: bool,
    /// Keep the process running and send queries over stdin (see `barkley_plugin`).
    #[serde(default)]
    pub persistent: bool,
    /// Extra environment for the process; `[plugin.<name>.env]` in config.toml overrides it.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

fn default_timeout() -> u64 {
    DEFAULT_TIMEOUT_MS
}

/// A running persistent plugin.
struct Persistent {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
}

pub struct ScriptPlugin {
    id: String,
    dir: PathBuf,
    manifest: Manifest,
    process: Mutex<Option<Persistent>>,
    next_request: AtomicU64,
}

impl ScriptPlugin {
    pub fn load(dir: &Path) -> Result<Self, String> {
        let path = dir.join("manifest.toml");
        let src = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let manifest: Manifest = toml::from_str(&src).map_err(|e| format!("{}: {e}", path.display()))?;
        let id = dir.file_name().map_or_else(|| manifest.name.clone(), |n| n.to_string_lossy().into_owned());
        Ok(Self { id, dir: dir.to_path_buf(), manifest, process: Mutex::new(None), next_request: AtomicU64::new(1) })
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Apply a `[plugin.<id>]` section from config.toml.
    pub fn apply(&mut self, o: &PluginOverride) {
        if let Some(k) = &o.keyword {
            self.manifest.keyword = (!k.is_empty()).then(|| k.clone());
        }
        self.manifest.env.extend(o.env.iter().map(|(k, v)| (k.clone(), v.clone())));
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(self.dir.join(&self.manifest.run));
        cmd.envs(&self.manifest.env).current_dir(&self.dir).process_group(0);
        cmd
    }

    fn timeout(&self) -> Duration {
        Duration::from_millis(self.manifest.timeout_ms)
    }

    fn run_once(&self, arg: &str) -> Result<Vec<Item>, String> {
        let exe = self.dir.join(&self.manifest.run);
        let mut child = self
            .command()
            .arg(arg)
            .env("BARKLEY_QUERY", arg)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot start {}: {e}", exe.display()))?;

        let mut stdout = child.stdout.take().expect("piped");
        let mut stderr = child.stderr.take().expect("piped");
        let out = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = (&mut stdout).take(MAX_OUTPUT).read_to_end(&mut buf);
            buf
        });
        let err = std::thread::spawn(move || {
            let mut buf = String::new();
            let _ = (&mut stderr).take(4096).read_to_string(&mut buf);
            buf
        });

        let deadline = Instant::now() + self.timeout();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() >= deadline => {
                    kill_group(child.id());
                    let _ = child.wait();
                    return Err(format!("timed out after {} ms", self.manifest.timeout_ms));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(1)),
                Err(e) => return Err(e.to_string()),
            }
        };

        let stdout = out.join().unwrap_or_default();
        let stderr = err.join().unwrap_or_default();
        if !status.success() {
            let msg = stderr.lines().next().unwrap_or("").trim();
            return Err(format!("exited with {status}{}{msg}", if msg.is_empty() { "" } else { ": " }));
        }
        parse_output(&stdout, &self.dir)
    }

    fn spawn_persistent(&self) -> Result<Persistent, String> {
        let exe = self.dir.join(&self.manifest.run);
        let mut child = self
            .command()
            .env("BARKLEY_PERSISTENT", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("cannot start {}: {e}", exe.display()))?;
        let stdin = child.stdin.take().expect("piped");
        let stdout = child.stdout.take().expect("piped");
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(Persistent { child, stdin, lines })
    }

    fn run_persistent(&self, arg: &str) -> Result<Vec<Item>, String> {
        let mut slot = self.process.lock().unwrap_or_else(|e| e.into_inner());
        if slot.is_none() {
            *slot = Some(self.spawn_persistent()?);
        }
        let p = slot.as_mut().expect("just set");

        let id = self.next_request.fetch_add(1, Ordering::Relaxed);
        let req = serde_json::to_string(&proto::Request { id, query: arg.to_string() }).expect("serializes");
        if writeln!(p.stdin, "{req}").and_then(|()| p.stdin.flush()).is_err() {
            stop(slot.take());
            return Err("plugin exited; restarting on next query".into());
        }

        let deadline = Instant::now() + self.timeout();
        loop {
            match p.lines.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(line) => match serde_json::from_str::<proto::Response>(&line) {
                    Ok(resp) if resp.id == id => return Ok(convert(resp.items, &self.dir)),
                    Ok(_) => {} // answer to an earlier, abandoned request
                    Err(e) => return Err(format!("invalid JSON output: {e}")),
                },
                Err(RecvTimeoutError::Timeout) => {
                    stop(slot.take());
                    return Err(format!("timed out after {} ms", self.manifest.timeout_ms));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    stop(slot.take());
                    return Err("plugin exited; restarting on next query".into());
                }
            }
        }
    }
}

impl Drop for ScriptPlugin {
    fn drop(&mut self) {
        stop(self.process.get_mut().ok().and_then(Option::take));
    }
}

fn stop(p: Option<Persistent>) {
    if let Some(mut p) = p {
        kill_group(p.child.id());
        let _ = p.child.wait();
    }
}

/// Parse one-shot stdout JSON.
pub fn parse_output(stdout: &[u8], dir: &Path) -> Result<Vec<Item>, String> {
    let out: proto::Output = serde_json::from_slice(stdout).map_err(|e| format!("invalid JSON output: {e}"))?;
    Ok(convert(out.items, dir))
}

/// Protocol items → app items. Items without a score rank in printed order.
fn convert(items: Vec<proto::Item>, dir: &Path) -> Vec<Item> {
    let n = items.len() as u32;
    items
        .into_iter()
        .enumerate()
        .map(|(i, it)| Item {
            title: it.title,
            subtitle: it.subtitle,
            score: it.score.unwrap_or(n - i as u32),
            action: match it.action {
                None => Action::Nothing,
                Some(proto::Action::OpenUrl { url }) => Action::OpenUrl(url),
                Some(proto::Action::Copy { text }) => Action::Copy(text),
                Some(proto::Action::Run { cmd, args }) => Action::Run { cmd, args },
            },
            icon: it.icon.map(|i| match i {
                proto::Icon::Symbol { symbol } => Icon::Symbol(symbol),
                proto::Icon::Path { path } => Icon::File(dir.join(path)),
            }),
            verb: it.verb,
            tag: it.tag,
            is_error: false,
        })
        .collect()
}

fn kill_group(pid: u32) {
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    const SIGKILL: i32 = 9;
    // SAFETY: plain syscall; a negative pid targets the process group we created.
    unsafe {
        kill(-(pid as i32), SIGKILL);
    }
}

impl Plugin for ScriptPlugin {
    fn id(&self) -> &str {
        &self.id
    }

    fn keyword(&self) -> Option<&str> {
        self.manifest.keyword.as_deref()
    }

    fn is_fallback(&self) -> bool {
        self.manifest.fallback
    }

    fn query(&self, q: &Query) -> Vec<Item> {
        // Called via our keyword → just the args; as a general/fallback plugin → everything.
        let arg = if q.keyword.is_some() && q.keyword.as_deref() == self.keyword() { q.args.as_str() } else { q.text() };
        let result = if self.manifest.persistent { self.run_persistent(arg) } else { self.run_once(arg) };
        result.unwrap_or_else(|e| vec![Item::error(format!("{}: plugin error", self.manifest.name), e)])
    }
}

/// Load `<dir>/*/manifest.toml` from each dir in order; a later folder with
/// the same name replaces an earlier one (user plugins override bundled ones).
/// Broken plugins are reported, not fatal.
pub fn load_all(dirs: &[PathBuf], overrides: &BTreeMap<String, PluginOverride>) -> (Vec<ScriptPlugin>, Vec<String>) {
    let mut found: BTreeMap<String, PathBuf> = BTreeMap::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for path in entries.flatten().map(|e| e.path()).filter(|p| p.join("manifest.toml").is_file()) {
            if let Some(name) = path.file_name() {
                found.insert(name.to_string_lossy().into_owned(), path);
            }
        }
    }

    let mut plugins = Vec::new();
    let mut errors = Vec::new();
    for (name, dir) in found {
        let o = overrides.get(&name);
        if o.is_some_and(|o| o.enabled == Some(false)) {
            continue;
        }
        match ScriptPlugin::load(&dir) {
            Ok(mut p) => {
                if let Some(o) = o {
                    p.apply(o);
                }
                plugins.push(p);
            }
            Err(e) => errors.push(e),
        }
    }
    (plugins, errors)
}

// `process_group` lives on the Unix extension trait.
use std::os::unix::process::CommandExt as _;

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn make_plugin(name: &str, manifest: &str, script: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("barkley-script-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("manifest.toml"), manifest).unwrap();
        let run = dir.join("run.sh");
        std::fs::write(&run, script).unwrap();
        std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o755)).unwrap();
        dir
    }

    const ECHO: &str = r#"#!/bin/sh
printf '{"items":[{"title":"%s","subtitle":"env=%s %s","action":{"type":"copy","text":"%s"}}]}' "$1" "$BARKLEY_QUERY" "$GREETING" "$1"
"#;

    /// Persistent mode in sh: answer each request line with its id and query.
    const PERSISTENT: &str = r#"#!/bin/sh
[ "$BARKLEY_PERSISTENT" = 1 ] || exit 9
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed 's/.*"id":\([0-9]*\).*/\1/')
  q=$(printf '%s' "$line" | sed 's/.*"query":"\([^"]*\)".*/\1/')
  [ "$q" = slow ] && sleep 5
  printf '{"id":%s,"items":[{"title":"%s pid=%s"}]}\n' "$id" "$q" "$$"
done
"#;

    #[test]
    fn parses_output_shapes() {
        let items = parse_output(
            br#"{"items":[
                {"title":"a","action":{"type":"open_url","url":"https://x"},"icon":{"symbol":"star"}},
                {"title":"b","subtitle":"s","action":{"type":"run","cmd":"say","args":["hi"]}},
                {"title":"c","score":99,"icon":{"path":"i.png"}}
            ]}"#,
            Path::new("/p"),
        )
        .unwrap();
        assert_eq!(items[0].action, Action::OpenUrl("https://x".into()));
        assert_eq!(items[0].icon, Some(Icon::Symbol("star".into())));
        assert_eq!(items[1].action, Action::Run { cmd: "say".into(), args: vec!["hi".into()] });
        assert_eq!(items[1].icon, None);
        assert_eq!(items[2].action, Action::Nothing);
        assert_eq!(items[2].icon, Some(Icon::File("/p/i.png".into())));
        assert_eq!(items.iter().map(|i| i.score).collect::<Vec<_>>(), [3, 2, 99]);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_output(b"hello", Path::new("/")).is_err());
        assert!(parse_output(br#"{"items":[{"title":"x","action":{"type":"nope"}}]}"#, Path::new("/")).is_err());
    }

    #[test]
    fn runs_script_with_argv_and_env() {
        let dir = make_plugin(
            "echo",
            "name = \"Echo\"\nkeyword = \"echo\"\nrun = \"./run.sh\"\n[env]\nGREETING = \"hey\"\n",
            ECHO,
        );
        let p = ScriptPlugin::load(&dir).unwrap();
        assert_eq!(p.id(), dir.file_name().unwrap().to_str().unwrap());
        assert_eq!(p.keyword(), Some("echo"));
        assert_eq!(p.manifest().timeout_ms, DEFAULT_TIMEOUT_MS);
        let items = p.query(&Query::parse("echo hi there"));
        assert_eq!(items.len(), 1, "{items:?}");
        assert_eq!(items[0].title, "hi there");
        assert_eq!(items[0].subtitle, "env=hi there hey");
        assert_eq!(items[0].action, Action::Copy("hi there".into()));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn fallback_gets_full_text_keyword_gets_args() {
        let dir = make_plugin("fb", "name = \"F\"\nkeyword = \"g\"\nfallback = true\nrun = \"./run.sh\"\n", ECHO);
        let p = ScriptPlugin::load(&dir).unwrap();
        assert!(p.is_fallback());
        assert_eq!(p.query(&Query::parse("g rust lang"))[0].title, "rust lang");
        assert_eq!(p.query(&Query::parse("rust lang"))[0].title, "rust lang");
        assert_eq!(p.query(&Query::parse("x rust"))[0].title, "x rust");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn overrides_keyword_and_env() {
        let dir = make_plugin("ovr", "name = \"O\"\nkeyword = \"echo\"\nrun = \"./run.sh\"\n", ECHO);
        let mut p = ScriptPlugin::load(&dir).unwrap();
        p.apply(&PluginOverride {
            keyword: Some("e".into()),
            env: [("GREETING".to_string(), "yo".to_string())].into(),
            ..Default::default()
        });
        assert_eq!(p.keyword(), Some("e"));
        assert_eq!(p.query(&Query::parse("e x"))[0].subtitle, "env=x yo");
        p.apply(&PluginOverride { keyword: Some(String::new()), ..Default::default() });
        assert_eq!(p.keyword(), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn persistent_plugin_reuses_one_process() {
        let dir = make_plugin("persist", "name = \"P\"\nkeyword = \"p\"\npersistent = true\nrun = \"./run.sh\"\n", PERSISTENT);
        let p = ScriptPlugin::load(&dir).unwrap();
        let a = p.query(&Query::parse("p one")).remove(0).title;
        let b = p.query(&Query::parse("p two")).remove(0).title;
        assert!(a.starts_with("one pid="), "{a}");
        assert!(b.starts_with("two pid="), "{b}");
        assert_eq!(a.split("pid=").nth(1), b.split("pid=").nth(1), "same process");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn persistent_timeout_restarts_process() {
        let dir = make_plugin(
            "persist-slow",
            "name = \"P\"\nkeyword = \"p\"\npersistent = true\ntimeout_ms = 200\nrun = \"./run.sh\"\n",
            PERSISTENT,
        );
        let p = ScriptPlugin::load(&dir).unwrap();
        let first = p.query(&Query::parse("p fast")).remove(0).title;
        let started = Instant::now();
        let slow = p.query(&Query::parse("p slow")).remove(0);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(slow.is_error && slow.subtitle.contains("timed out"), "{slow:?}");
        let again = p.query(&Query::parse("p fast")).remove(0).title;
        assert!(again.starts_with("fast pid="));
        assert_ne!(first.split("pid=").nth(1), again.split("pid=").nth(1), "respawned");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn timeout_yields_error_item() {
        let dir = make_plugin(
            "slow",
            "name = \"Slow\"\nkeyword = \"slow\"\nrun = \"./run.sh\"\ntimeout_ms = 100\n",
            "#!/bin/sh\nsleep 5\n",
        );
        let started = Instant::now();
        let items = ScriptPlugin::load(&dir).unwrap().query(&Query::parse("slow x"));
        assert!(started.elapsed() < Duration::from_secs(2), "took {:?}", started.elapsed());
        assert!(items[0].is_error);
        assert!(items[0].subtitle.contains("timed out"), "{}", items[0].subtitle);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failing_script_yields_error_item() {
        let dir = make_plugin("fail", "name = \"Fail\"\nrun = \"./run.sh\"\n", "#!/bin/sh\necho boom >&2\nexit 3\n");
        let items = ScriptPlugin::load(&dir).unwrap().query(&Query::parse("x"));
        assert!(items[0].is_error);
        assert!(items[0].subtitle.contains("boom"), "{}", items[0].subtitle);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_executable_yields_error_item() {
        let dir = make_plugin("missing", "name = \"Missing\"\nrun = \"./nope\"\n", "");
        let items = ScriptPlugin::load(&dir).unwrap().query(&Query::parse("x"));
        assert!(items[0].is_error);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn load_all_merges_dirs_overrides_and_reports_errors() {
        let root = std::env::temp_dir().join(format!("barkley-loadall-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (bundled, user) = (root.join("bundled"), root.join("user"));
        for d in ["bundled/good", "bundled/shadowed", "bundled/off", "user/shadowed", "user/bad", "user/not-a-plugin"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        std::fs::write(bundled.join("good/manifest.toml"), "name = \"G\"\nrun = \"./x\"\n").unwrap();
        std::fs::write(bundled.join("shadowed/manifest.toml"), "name = \"Bundled\"\nrun = \"./x\"\n").unwrap();
        std::fs::write(bundled.join("off/manifest.toml"), "name = \"Off\"\nrun = \"./x\"\n").unwrap();
        std::fs::write(user.join("shadowed/manifest.toml"), "name = \"User\"\nrun = \"./x\"\n").unwrap();
        std::fs::write(user.join("bad/manifest.toml"), "name = 1\n").unwrap();

        let overrides = [("off".to_string(), PluginOverride { enabled: Some(false), ..Default::default() })].into();
        let (plugins, errors) = load_all(&[bundled, user], &overrides);
        let names: Vec<_> = plugins.iter().map(|p| (p.id(), p.manifest().name.as_str())).collect();
        assert_eq!(names, [("good", "G"), ("shadowed", "User")]);
        assert_eq!(errors.len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }
}
