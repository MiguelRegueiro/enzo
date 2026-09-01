use std::{
    env,
    ffi::{OsStr, OsString},
    process::{Command, Stdio},
};

pub(crate) fn looks_like_kitty() -> bool {
    env::var("TERM")
        .map(|term| term.to_ascii_lowercase().contains("kitty"))
        .unwrap_or(false)
        || env::var_os("KITTY_WINDOW_ID").is_some()
        || env::var("TERM_PROGRAM")
            .map(|term| term.eq_ignore_ascii_case("kitty"))
            .unwrap_or(false)
}

pub(crate) fn inside_tmux() -> bool {
    env::var_os("TMUX").is_some()
}

pub(crate) fn enable_tmux_passthrough() {
    if !inside_tmux() {
        return;
    }

    let args = allow_passthrough_args(env::var_os("TMUX_PANE").as_deref());
    let _ = Command::new("tmux")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn allow_passthrough_args(target_pane: Option<&OsStr>) -> Vec<OsString> {
    let mut args = ["set-option", "-p", "-q"]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
    if let Some(pane) = target_pane
        && !pane.is_empty()
    {
        args.push("-t".into());
        args.push(pane.into());
    }
    args.extend(["allow-passthrough", "on"].into_iter().map(Into::into));
    args
}

#[cfg(test)]
#[path = "tests/terminal_detection.rs"]
mod tests;
