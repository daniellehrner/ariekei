//! The processes an agent started, e.g. Claude Code's background shells: closing the agent
//! would end them, and resuming the conversation does not start them again.

use std::process::Command;

pub struct Process {
    pub pid: u32,
    pub parent: u32,
    pub name: String,
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
        "-A", "-o", "pid=", "-o", "ppid=", "-o", "stat=", "-o", "comm=",
    ]);
    cmd
}

#[cfg(windows)]
fn list_command() -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut cmd = Command::new("powershell");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "Get-CimInstance Win32_Process | ForEach-Object { '{0} {1} R {2}' -f $_.ProcessId, $_.ParentProcessId, $_.Name }",
    ])
    .creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// Lines of `pid ppid state name`; zombies (state Z, ended but not yet reaped) are left out.
fn parse(text: &str) -> Vec<Process> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse().ok()?;
            let parent = fields.next()?.parse().ok()?;
            if fields.next()?.starts_with('Z') {
                return None;
            }
            let name = fields.collect::<Vec<_>>().join(" ");
            Some(Process { pid, parent, name })
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
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(names, ["bash", "sleep"]);
        assert!(descendants(&table, 200).is_empty());
    }

    #[test]
    fn names_may_contain_spaces() {
        let table = parse("7 1 S /Applications/Some App.app/x\n");
        assert_eq!(table[0].name, "/Applications/Some App.app/x");
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
                .map(|p| (p.pid, p.name.clone()))
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
