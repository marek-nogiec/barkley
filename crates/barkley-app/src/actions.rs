use std::io::Read;
use std::process::{Command, Stdio};

use barkley_core::Action;
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString, NSWorkspace};
use objc2_foundation::{NSString, NSURL};

/// `what` names the action in error messages, e.g. "Connect Studio Buds".
pub fn perform(action: &Action, what: String) {
    match action {
        Action::OpenUrl(url) => open_url(url),
        Action::Copy(text) => copy(text),
        Action::Run { cmd, args } => run(cmd, args, what),
        Action::Nothing => {}
    }
}

fn open_url(url: &str) {
    let Some(nsurl) = NSURL::URLWithString(&NSString::from_str(url)) else {
        eprintln!("barkley: invalid URL {url:?}");
        return;
    };
    if !NSWorkspace::sharedWorkspace().openURL(&nsurl) {
        eprintln!("barkley: no app to open {url:?}");
    }
}

fn copy(text: &str) {
    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();
    pb.setString_forType(&NSString::from_str(text), unsafe { NSPasteboardTypeString });
}

/// Runs in the background. If the command fails, the panel reopens with its
/// last stderr line, so e.g. "headphones out of range" isn't silent.
pub fn run(cmd: &str, args: &[String], what: String) {
    let child = Command::new(cmd).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return report(what, format!("can't start {cmd}: {e}")),
    };
    std::thread::spawn(move || {
        let mut stderr = String::new();
        if let Some(mut pipe) = child.stderr.take() {
            let _ = (&mut pipe).take(8192).read_to_string(&mut stderr);
        }
        match child.wait() {
            Ok(status) if status.success() => {}
            Ok(status) => report(what, failure_detail(&stderr).unwrap_or_else(|| format!("exited with {status}"))),
            Err(e) => report(what, e.to_string()),
        }
    });
}

/// Last non-empty stderr line, minus a leading `program: ` prefix.
fn failure_detail(stderr: &str) -> Option<String> {
    let line = stderr.lines().map(str::trim).rfind(|l| !l.is_empty())?;
    let line = match line.split_once(": ") {
        Some((prog, rest)) if !prog.contains(' ') => rest,
        _ => line,
    };
    Some(line.to_string())
}

fn report(what: String, detail: String) {
    eprintln!("barkley: {what} failed: {detail}");
    dispatch2::DispatchQueue::main().exec_async(move || crate::controller::action_failed(&what, &detail));
}

#[cfg(test)]
mod tests {
    use super::failure_detail;

    #[test]
    fn failure_detail_takes_last_line_without_program_prefix() {
        assert_eq!(failure_detail("warn\nbarkley-audio: couldn't connect Studio Buds\n\n").as_deref(), Some("couldn't connect Studio Buds"));
        assert_eq!(failure_detail("Error: x: y").as_deref(), Some("x: y"));
        assert_eq!(failure_detail("no prefix here").as_deref(), Some("no prefix here"));
        assert_eq!(failure_detail(" \n"), None);
    }
}
