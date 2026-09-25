// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Gives subprocesses the user's login-shell environment, not the desktop launcher's sparse one.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::io::{IsTerminal, Read};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, LazyLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const RESOLVE_TIMEOUT: Duration = Duration::from_secs(5);
const RESOLVING_VAR: &str = "PUSHGIT_RESOLVING_ENVIRONMENT";
const DISABLE_VAR: &str = "PUSHGIT_SHELL_ENV";
const EXCLUDED_KEYS: &[&str] = &[
    "PWD",
    "OLDPWD",
    "SHLVL",
    "_",
    "TERM",
    "COLORTERM",
    RESOLVING_VAR,
];

type Env = HashMap<OsString, OsString>;

static RESOLVED: LazyLock<Option<Env>> = LazyLock::new(resolve_for_app);

pub fn resolve_in_background() {
    std::thread::spawn(|| {
        LazyLock::force(&RESOLVED);
    });
}

#[allow(clippy::disallowed_methods)]
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    if let Some(env) = RESOLVED.as_ref() {
        command.envs(env);
    }
    command
}

pub fn async_command(program: impl AsRef<OsStr>) -> tokio::process::Command {
    command(program).into()
}

// Values are never logged: environments routinely carry tokens.
fn resolve_for_app() -> Option<Env> {
    if cfg!(test) {
        return None;
    }
    if std::env::var_os(DISABLE_VAR).is_some_and(|value| value == "0") {
        eprintln!("pushgit: shell environment disabled via {DISABLE_VAR}=0");
        return None;
    }
    if std::io::stdin().is_terminal() {
        eprintln!("pushgit: shell environment not resolved: launched from a terminal");
        return None;
    }
    let Some(home) = std::env::var_os("HOME") else {
        eprintln!("pushgit: shell environment not resolved: HOME is unset");
        return None;
    };

    let shell = user_shell();
    let started = Instant::now();
    match resolve(&shell, Path::new(&home), RESOLVE_TIMEOUT) {
        Ok(env) => {
            eprintln!(
                "pushgit: shell environment resolved in {} ms via {}",
                started.elapsed().as_millis(),
                shell.display()
            );
            Some(env)
        }
        Err(reason) => {
            eprintln!(
                "pushgit: shell environment not resolved via {}: {reason}",
                shell.display()
            );
            None
        }
    }
}

fn user_shell() -> PathBuf {
    std::env::var_os("SHELL")
        .filter(|shell| !shell.is_empty())
        .map(PathBuf::from)
        .or_else(passwd_shell)
        .unwrap_or_else(|| PathBuf::from("/bin/sh"))
}

fn passwd_shell() -> Option<PathBuf> {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .ok()?;
    let passwd = std::fs::read_to_string("/etc/passwd").ok()?;
    passwd_shell_for(&passwd, &user)
}

fn passwd_shell_for(passwd: &str, user: &str) -> Option<PathBuf> {
    passwd.lines().find_map(|line| {
        let fields: Vec<&str> = line.split(':').collect();
        (fields.len() == 7 && fields[0] == user && !fields[6].is_empty())
            .then(|| PathBuf::from(fields[6]))
    })
}

// Runs in `home`, never a repo: prompt frameworks and direnv execute repo-controlled config on shell start.
#[allow(clippy::disallowed_methods)]
fn resolve(shell: &Path, home: &Path, timeout: Duration) -> Result<Env, String> {
    let marker = unique_marker();
    let script = format!("printf '%s' '{marker}'; command env -0; printf '%s' '{marker}'");

    let mut child = Command::new(shell)
        .args(["-i", "-c", &script])
        .current_dir(home)
        .env(RESOLVING_VAR, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|e| format!("failed to start: {e}"))?;

    let stdout = child.stdout.take().expect("stdout is piped");
    let (sender, receiver) = mpsc::channel();
    let reader_marker = marker.clone().into_bytes();
    std::thread::spawn(move || {
        let _ = sender.send(read_until_closing_marker(stdout, &reader_marker));
    });

    let Ok(output) = receiver.recv_timeout(timeout) else {
        kill_process_group(&child);
        let _ = child.wait();
        return Err(format!("timed out after {} s", timeout.as_secs()));
    };

    // Background jobs from the rc file may keep the shell's group alive; reap without blocking on them.
    std::thread::spawn(move || {
        let _ = child.wait();
    });

    parse_env_output(&output, marker.as_bytes()).ok_or_else(|| "markers missing".to_string())
}

fn unique_marker() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!("__PUSHGIT_ENV_{}_{nanos}__", std::process::id())
}

// Stops at the closing marker rather than EOF: a daemon forked by the rc file can hold stdout open.
fn read_until_closing_marker(mut reader: impl Read, marker: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => output.extend_from_slice(&chunk[..n]),
        }
        if between_markers(&output, marker).is_some() {
            break;
        }
    }
    output
}

fn kill_process_group(child: &Child) {
    let Ok(pgid) = i32::try_from(child.id()) else {
        return;
    };
    // SAFETY: kill(2) takes no pointers; a negative pid targets the group `process_group(0)` created.
    unsafe {
        libc::kill(-pgid, libc::SIGKILL);
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn between_markers<'a>(output: &'a [u8], marker: &[u8]) -> Option<&'a [u8]> {
    let start = find(output, marker)? + marker.len();
    let len = find(&output[start..], marker)?;
    Some(&output[start..start + len])
}

fn parse_env_output(output: &[u8], marker: &[u8]) -> Option<Env> {
    let body = between_markers(output, marker)?;
    Some(
        body.split(|&byte| byte == 0)
            .filter_map(|entry| {
                let eq = entry.iter().position(|&byte| byte == b'=')?;
                let key = &entry[..eq];
                if key.is_empty() || EXCLUDED_KEYS.iter().any(|k| k.as_bytes() == key) {
                    return None;
                }
                Some((
                    OsStr::from_bytes(key).to_owned(),
                    OsStr::from_bytes(&entry[eq + 1..]).to_owned(),
                ))
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    const MARKER: &[u8] = b"@@M@@";

    fn get<'a>(env: &'a Env, key: &str) -> Option<&'a str> {
        env.get(OsStr::new(key)).and_then(|v| v.to_str())
    }

    fn write_script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn parse_ignores_noise_outside_the_markers() {
        let output = b"Now using node v24\n@@M@@FOO=bar\0@@M@@trailing noise";
        let env = parse_env_output(output, MARKER).unwrap();
        assert_eq!(env.len(), 1);
        assert_eq!(get(&env, "FOO"), Some("bar"));
    }

    #[test]
    fn parse_keeps_multi_line_values_and_values_containing_equals() {
        let output = b"@@M@@MULTI=line one\nline two\0OPTS=a=b=c\0@@M@@";
        let env = parse_env_output(output, MARKER).unwrap();
        assert_eq!(get(&env, "MULTI"), Some("line one\nline two"));
        assert_eq!(get(&env, "OPTS"), Some("a=b=c"));
    }

    #[test]
    fn parse_fails_without_both_markers() {
        assert!(parse_env_output(b"", MARKER).is_none());
        assert!(parse_env_output(b"FOO=bar\0", MARKER).is_none());
        assert!(parse_env_output(b"@@M@@FOO=bar\0", MARKER).is_none());
    }

    #[test]
    fn parse_drops_per_shell_instance_keys() {
        let output =
            b"@@M@@PWD=/x\0OLDPWD=/y\0SHLVL=2\0_=/usr/bin/env\0TERM=xterm\0COLORTERM=truecolor\0PUSHGIT_RESOLVING_ENVIRONMENT=1\0PATH=/bin\0@@M@@";
        let env = parse_env_output(output, MARKER).unwrap();
        assert_eq!(env.len(), 1);
        assert_eq!(get(&env, "PATH"), Some("/bin"));
    }

    #[test]
    fn passwd_shell_is_read_from_the_matching_user() {
        let passwd = "root:x:0:0:root:/root:/bin/bash\ntom:x:1000:1000::/home/tom:/usr/bin/zsh\n";
        assert_eq!(
            passwd_shell_for(passwd, "tom"),
            Some(PathBuf::from("/usr/bin/zsh"))
        );
        assert_eq!(passwd_shell_for(passwd, "nobody"), None);
    }

    #[test]
    fn resolve_captures_the_environment_set_up_by_the_shell() {
        let dir = tempfile::tempdir().unwrap();
        let shell = write_script(
            dir.path(),
            "fake-shell",
            "echo 'Now using node v24'\nexport FOO=bar\nexport PATH=\"/opt/fake/bin:$PATH\"\nexec /bin/sh -c \"$3\"",
        );

        let env = resolve(&shell, dir.path(), RESOLVE_TIMEOUT).unwrap();

        assert_eq!(get(&env, "FOO"), Some("bar"));
        assert!(get(&env, "PATH").unwrap().starts_with("/opt/fake/bin:"));
    }

    #[test]
    fn resolve_runs_in_home_with_the_resolving_flag_as_an_interactive_non_login_shell() {
        let dir = tempfile::tempdir().unwrap();
        let shell = write_script(
            dir.path(),
            "fake-shell",
            "export SEEN_CWD=\"$(pwd -P)\" SEEN_FLAG=\"$PUSHGIT_RESOLVING_ENVIRONMENT\" SEEN_ARGS=\"$1 $2\"\nexec /bin/sh -c \"$3\"",
        );

        let env = resolve(&shell, dir.path(), RESOLVE_TIMEOUT).unwrap();

        let home = dir.path().canonicalize().unwrap();
        assert_eq!(get(&env, "SEEN_CWD"), home.to_str());
        assert_eq!(get(&env, "SEEN_FLAG"), Some("1"));
        assert_eq!(get(&env, "SEEN_ARGS"), Some("-i -c"));
    }

    #[test]
    fn resolve_fails_when_the_shell_exits_without_printing_the_environment() {
        let dir = tempfile::tempdir().unwrap();
        let shell = write_script(dir.path(), "fake-shell", "exit 1");

        assert!(resolve(&shell, dir.path(), RESOLVE_TIMEOUT).is_err());
    }

    #[test]
    fn resolve_fails_when_the_shell_cannot_be_started() {
        let dir = tempfile::tempdir().unwrap();

        assert!(resolve(&dir.path().join("missing"), dir.path(), RESOLVE_TIMEOUT).is_err());
    }

    #[test]
    fn resolve_times_out_and_kills_the_shell_s_process_group() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("background.pid");
        let shell = write_script(
            dir.path(),
            "fake-shell",
            &format!("sleep 30 &\necho $! > '{}'\nwait", pid_file.display()),
        );

        let started = Instant::now();
        let result = resolve(&shell, dir.path(), Duration::from_millis(300));

        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(5));
        let background: i32 = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        std::thread::sleep(Duration::from_millis(100));
        // SAFETY: signal 0 only probes for existence.
        let alive = unsafe { libc::kill(background, 0) } == 0;
        assert!(
            !alive,
            "background job from the rc file outlived the timeout"
        );
    }

    #[test]
    fn resolve_returns_as_soon_as_the_environment_is_printed_even_if_a_daemon_holds_stdout() {
        let dir = tempfile::tempdir().unwrap();
        let shell = write_script(
            dir.path(),
            "fake-shell",
            "sleep 3 &\nexec /bin/sh -c \"$3\"",
        );

        let started = Instant::now();
        let env = resolve(&shell, dir.path(), RESOLVE_TIMEOUT);

        assert!(env.is_ok());
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn a_command_finds_its_program_on_an_overridden_path() {
        let dir = tempfile::tempdir().unwrap();
        write_script(dir.path(), "pushgit-path-probe", "exit 0");

        let status = Command::new("pushgit-path-probe")
            .env("PATH", dir.path())
            .status()
            .unwrap();

        assert!(status.success());
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn a_per_call_env_overrides_a_resolved_value() {
        let resolved: Env = [(OsString::from("GIT_DIR"), OsString::from("/resolved"))].into();
        let output = Command::new("sh")
            .args(["-c", "printf %s \"$GIT_DIR\""])
            .envs(&resolved)
            .env("GIT_DIR", "/per-call")
            .output()
            .unwrap();

        assert_eq!(output.stdout, b"/per-call");
    }
}
