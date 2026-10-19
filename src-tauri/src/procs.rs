// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

//! The processes an agent started, e.g. Claude Code's background shells: closing the agent
//! would end them, and resuming the conversation does not start them again.

use std::process::Command;

pub struct Process {
    pub pid: u32,
    pub parent: u32,
    /// The command line (on Windows the name, when the command line cannot be read).
    pub command: String,
}

/// The running processes, from `ps` (Unix) or `Win32_Process` (Windows).
pub fn table() -> Result<Vec<Process>, String> {
    let out = list_command().output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
    }
    Ok(parse(&String::from_utf8_lossy(&out.stdout)))
}

#[cfg(unix)]
fn list_command() -> Command {
    // One field per -o: BSD ps (macOS) takes everything after `=` as the header.
    let mut cmd = Command::new("ps");
    cmd.args([
        "-A", "-o", "pid=", "-o", "ppid=", "-o", "stat=", "-o", "args=",
    ]);
    cmd
}

#[cfg(windows)]
fn list_command() -> Command {
    let mut cmd = crate::tools::quiet("powershell");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "Get-CimInstance Win32_Process | ForEach-Object { '{0} {1} R {2}' -f $_.ProcessId, $_.ParentProcessId, ($(if ($_.CommandLine) { $_.CommandLine -replace '\\s+', ' ' } else { $_.Name })) }",
    ]);
    cmd
}

/// Lines of `pid ppid state command`; zombies (state Z, ended but not yet reaped) are left out.
fn parse(text: &str) -> Vec<Process> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse().ok()?;
            let parent = fields.next()?.parse().ok()?;
            if fields.next()?.starts_with('Z') {
                return None;
            }
            let command = fields.collect::<Vec<_>>().join(" ");
            Some(Process {
                pid,
                parent,
                command,
            })
        })
        .collect()
}

/// The processes below `root`, children first.
pub fn descendants(table: &[Process], root: u32) -> Vec<&Process> {
    let mut found: Vec<&Process> = vec![];
    let mut parents = vec![root];
    while let Some(parent) = parents.pop() {
        for p in table.iter().filter(|p| p.parent == parent && p.pid != root) {
            if !found.iter().any(|f| f.pid == p.pid) {
                found.push(p);
                parents.push(p.pid);
            }
        }
    }
    found
}

/// What `root` started itself (the first process of each tree below it), e.g. one per
/// background shell rather than the shell and everything it runs.
pub fn started_by(table: &[Process], root: u32) -> Vec<&Process> {
    let below = descendants(table, root);
    below
        .iter()
        .copied()
        .filter(|p| p.parent == root || !below.iter().any(|q| q.pid == p.parent))
        .collect()
}

/// The command a user would recognize: Claude Code runs a Bash call as `bash -c -l "source
/// <snapshot> && eval '<command>' …"`, so the quoted command when there is one.
pub fn shown_command(command: &str) -> String {
    let Some(start) = command.find("eval '") else {
        return command.to_owned();
    };
    // A single-quoted shell word: `'\''` is a quote inside it.
    let mut out = String::new();
    let mut rest = &command[start + "eval '".len()..];
    loop {
        match rest.find('\'') {
            None => return command.to_owned(),
            Some(i) => {
                out.push_str(&rest[..i]);
                rest = &rest[i + 1..];
                if let Some(after) = rest.strip_prefix("\\''") {
                    out.push('\'');
                    rest = after;
                } else {
                    return out;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn finds_children_and_grandchildren_but_no_zombies() {
        let table = parse(
            "  1     0 Ss   init\n\
             100     1 Sl   claude\n\
             101   100 S    bash\n\
             102   101 S    sleep\n\
             103   100 Z    bash\n\
             200     1 S    other\n\
             garbage\n",
        );
        let names: Vec<&str> = descendants(&table, 100)
            .iter()
            .map(|p| p.command.as_str())
            .collect();
        assert_eq!(names, ["bash", "sleep"]);
        assert!(descendants(&table, 200).is_empty());
        let started: Vec<u32> = started_by(&table, 100).iter().map(|p| p.pid).collect();
        assert_eq!(started, [101], "the shell, not what it runs");
    }

    #[test]
    fn commands_keep_their_spaces() {
        let table = parse("7 1 S /Applications/Some App.app/x --flag\n");
        assert_eq!(table[0].command, "/Applications/Some App.app/x --flag");
    }

    #[test]
    fn shows_the_command_inside_claude_codes_shell() {
        assert_eq!(
            shown_command(
                "/bin/bash -c -l source /home/me/.claude/shell-snapshots/snapshot-bash-1.sh && \
                 eval 'npm run dev -- --port 5173' \\< /dev/null && pwd -P >| /tmp/claude-1-cwd"
            ),
            "npm run dev -- --port 5173"
        );
        assert_eq!(
            shown_command("bash -c -l eval 'echo '\\''hi'\\'' && sleep 9' < /dev/null"),
            "echo 'hi' && sleep 9"
        );
        assert_eq!(
            shown_command("python3 -m http.server"),
            "python3 -m http.server"
        );
        assert_eq!(
            shown_command("sh -c eval 'unclosed"),
            "sh -c eval 'unclosed"
        );
    }

    #[test]
    fn sees_what_a_running_process_started() {
        #[cfg(unix)]
        let mut child = Command::new("sh")
            .args(["-c", "sleep 30; :"])
            .spawn()
            .unwrap();
        #[cfg(windows)]
        let mut child = Command::new("cmd")
            .args(["/c", "ping -n 31 127.0.0.1 >nul"])
            .spawn()
            .unwrap();
        // Generous: on a busy Windows runner PowerShell alone can take seconds to start.
        let start = Instant::now();
        let found: Vec<(u32, String)> = loop {
            let table = table().unwrap();
            let found: Vec<(u32, String)> = descendants(&table, child.id())
                .iter()
                .map(|p| (p.pid, p.command.clone()))
                .collect();
            if !found.is_empty() || start.elapsed() > Duration::from_secs(20) {
                break found;
            }
            std::thread::sleep(Duration::from_millis(200));
        };
        // Leave nothing running: the grandchild outlives its killed parent.
        for (pid, _) in &found {
            #[cfg(unix)]
            let _ = Command::new("kill").arg(pid.to_string()).status();
            #[cfg(windows)]
            let _ = Command::new("taskkill")
                .args(["/F", "/PID", &pid.to_string()])
                .output();
        }
        let _ = child.kill();
        let _ = child.wait();
        assert!(
            found
                .iter()
                .any(|(_, n)| n.contains("sleep") || n.contains("PING") || n.contains("ping")),
            "{found:?}"
        );
    }
}
