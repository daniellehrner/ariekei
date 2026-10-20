// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

//! An agent and the processes it starts get a memory cap of their own, outside Ariekei's
//! process group, so the OS stops a runaway agent without stopping Ariekei or the other
//! agents. By default the cap is half the RAM; `ARIEKEI_AGENT_MEMORY_MAX` sets another, in
//! bytes or with a K, M or G suffix.
//!
//! - Linux: a systemd scope per agent. Over the cap the kernel kills the agent's largest
//!   process (the runaway command), and the agent carries on. Without the scope, systemd-oomd
//!   would kill Ariekei's whole scope (app, webview, every agent) under memory pressure.
//! - Windows: a job object per agent. Over the cap, allocations in the agent's processes
//!   fail; without it, whichever process allocates next fails, Ariekei and WebView2 included.
//! - macOS: nothing. Nothing kills a process group there, and no per-process memory limit is
//!   enforced.

use std::ffi::OsString;

#[cfg(any(target_os = "linux", windows))]
const CAP_ENV: &str = "ARIEKEI_AGENT_MEMORY_MAX";

/// The program and arguments that start `program` with `prefix` (see `tools::program`)
/// confined; `unit` names the agent's systemd scope, `label` describes it.
#[cfg(target_os = "linux")]
pub fn wrap(
    program: OsString,
    prefix: Vec<OsString>,
    unit: &str,
    label: &str,
) -> (OsString, Vec<OsString>) {
    if !linux::available() {
        return (program, prefix);
    }
    let cap = std::env::var(CAP_ENV).unwrap_or_else(|_| "50%".to_owned());
    let mut args: Vec<OsString> = linux::scope_args(&cap, unit, label)
        .into_iter()
        .map(OsString::from)
        .collect();
    args.push(program);
    args.extend(prefix);
    ("systemd-run".into(), args)
}

#[cfg(not(target_os = "linux"))]
pub fn wrap(
    program: OsString,
    prefix: Vec<OsString>,
    _unit: &str,
    _label: &str,
) -> (OsString, Vec<OsString>) {
    (program, prefix)
}

/// Puts the just-started agent `pid` into a job object with the cap. Its children inherit the
/// job; a child started before this returns would not, but `claude` starts none that early.
#[cfg(windows)]
pub fn adopt(pid: Option<u32>) {
    let Some(pid) = pid else { return };
    let cap = match std::env::var(CAP_ENV) {
        Ok(v) => parse_size(&v),
        Err(_) => win::total_memory().map(|total| total / 2),
    };
    let Some(cap) = cap else {
        tracing::warn!("agent {pid} not confined: no memory cap");
        return;
    };
    if let Err(e) = win::confine(pid, cap) {
        tracing::warn!("agent {pid} not confined: {e}");
    }
}

#[cfg(not(windows))]
pub fn adopt(_pid: Option<u32>) {}

/// `1048576`, `512K`, `100M` or `2G` in bytes.
#[cfg(any(windows, test))]
fn parse_size(text: &str) -> Option<u64> {
    let text = text.trim();
    let (digits, unit) = match text.char_indices().last()? {
        (i, c) if c.is_ascii_alphabetic() => (&text[..i], c.to_ascii_uppercase()),
        _ => (text, 'B'),
    };
    let shift = match unit {
        'B' => 0,
        'K' => 10,
        'M' => 20,
        'G' => 30,
        _ => return None,
    };
    digits.parse::<u64>().ok()?.checked_mul(1 << shift)
}

#[cfg(target_os = "linux")]
mod linux {
    use std::sync::OnceLock;

    /// `systemd-run` arguments up to the command. `--scope` runs the command in place (same
    /// pid, terminal, environment and exit code); without swap the cap is reached instead of
    /// swapping; `OOMPolicy=continue` keeps the agent running when its command is killed.
    pub fn scope_args(cap: &str, unit: &str, label: &str) -> Vec<String> {
        vec![
            "--user".into(),
            "--scope".into(),
            "--quiet".into(),
            "--collect".into(),
            "--slice-inherit".into(),
            format!("--unit=ariekei-agent-{unit}"),
            format!("--description=Ariekei agent: {label}"),
            "--property".into(),
            format!("MemoryMax={cap}"),
            "--property".into(),
            "MemorySwapMax=0".into(),
            "--property".into(),
            "OOMPolicy=continue".into(),
            "--".into(),
        ]
    }

    /// Whether this session's systemd user manager starts such scopes: not without one (CI,
    /// the e2e tests' private D-Bus), nor before systemd 253 (no `OOMPolicy` for scopes).
    pub fn available() -> bool {
        static AVAILABLE: OnceLock<bool> = OnceLock::new();
        *AVAILABLE.get_or_init(|| {
            let out = crate::tools::quiet("systemd-run")
                .args(scope_args(
                    "50%",
                    &uuid::Uuid::new_v4().simple().to_string(),
                    "probe",
                ))
                .arg("true")
                .output();
            match out {
                Ok(out) if out.status.success() => {
                    tracing::info!("agents run in systemd scopes of their own");
                    true
                }
                Ok(out) => {
                    tracing::warn!(
                        "agents run in Ariekei's scope: systemd-run: {}",
                        String::from_utf8_lossy(&out.stderr).trim()
                    );
                    false
                }
                Err(e) => {
                    tracing::warn!("agents run in Ariekei's scope: systemd-run: {e}");
                    false
                }
            }
        })
    }
}

#[cfg(windows)]
mod win {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_JOB_MEMORY,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
    };

    pub fn total_memory() -> Option<u64> {
        // SAFETY: `status` is a MEMORYSTATUSEX with its length set, as the call requires.
        unsafe {
            let mut status: MEMORYSTATUSEX = std::mem::zeroed();
            status.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
            (GlobalMemoryStatusEx(&mut status) != 0).then_some(status.ullTotalPhys)
        }
    }

    /// Closes the handle when dropped. The job outlives its handle while it has processes.
    struct Handle(HANDLE);

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: the handle is open and owned by this value.
            unsafe { CloseHandle(self.0) };
        }
    }

    fn open(handle: HANDLE, what: &str) -> Result<Handle, String> {
        if handle.is_null() {
            Err(format!("{what}: {}", std::io::Error::last_os_error()))
        } else {
            Ok(Handle(handle))
        }
    }

    fn check(ok: i32, what: &str) -> Result<(), String> {
        if ok == 0 {
            Err(format!("{what}: {}", std::io::Error::last_os_error()))
        } else {
            Ok(())
        }
    }

    pub fn confine(pid: u32, cap: u64) -> Result<(), String> {
        // SAFETY: plain Win32 calls on handles this function owns; `limits` is the structure
        // JobObjectExtendedLimitInformation expects, with its size.
        unsafe {
            let job = open(
                CreateJobObjectW(std::ptr::null(), std::ptr::null()),
                "CreateJobObject",
            )?;
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_JOB_MEMORY;
            limits.JobMemoryLimit = usize::try_from(cap).unwrap_or(usize::MAX);
            check(
                SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    (&raw const limits).cast(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                ),
                "SetInformationJobObject",
            )?;
            let process = open(
                OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid),
                "OpenProcess",
            )?;
            check(
                AssignProcessToJobObject(job.0, process.0),
                "AssignProcessToJobObject",
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(parse_size("1048576"), Some(1 << 20));
        assert_eq!(parse_size("512K"), Some(512 << 10));
        assert_eq!(parse_size("100m"), Some(100 << 20));
        assert_eq!(parse_size(" 2G "), Some(2 << 30));
        assert_eq!(parse_size("50%"), None);
        assert_eq!(parse_size("M"), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn scope_arguments() {
        let args = linux::scope_args("100M", "abc", "acme/widgets#7");
        assert_eq!(args.first().map(String::as_str), Some("--user"));
        assert_eq!(args.last().map(String::as_str), Some("--"));
        for wanted in [
            "--scope",
            "--unit=ariekei-agent-abc",
            "--description=Ariekei agent: acme/widgets#7",
            "MemoryMax=100M",
            "MemorySwapMax=0",
            "OOMPolicy=continue",
        ] {
            assert!(
                args.iter().any(|a| a == wanted),
                "{wanted} missing: {args:?}"
            );
        }
    }

    /// Starts `script` (Python) confined to `cap`, as `Terminals::open` starts an agent.
    #[cfg(any(target_os = "linux", windows))]
    fn run_confined(script: &str, cap: &str) -> Option<std::process::Output> {
        let python = std::env::var_os("ARIEKEI_PYTHON").unwrap_or_else(|| "python3".into());
        let prefix: Vec<OsString> = vec!["-I".into(), "-c".into(), script.into()];
        #[cfg(target_os = "linux")]
        let (program, args) = {
            if !linux::available() {
                eprintln!("skipped: no systemd user manager");
                return None;
            }
            let unit = uuid::Uuid::new_v4().simple().to_string();
            let mut args: Vec<OsString> = linux::scope_args(cap, &unit, "test")
                .into_iter()
                .map(OsString::from)
                .collect();
            args.push(python);
            args.extend(prefix);
            (OsString::from("systemd-run"), args)
        };
        #[cfg(windows)]
        let (program, args) = (python, prefix);
        let child = crate::tools::quiet(program)
            .args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        #[cfg(windows)]
        win::confine(child.id(), parse_size(cap).unwrap()).unwrap();
        Some(child.wait_with_output().unwrap())
    }

    /// A command over the cap fails; the agent that ran it does not.
    #[cfg(any(target_os = "linux", windows))]
    #[test]
    fn over_the_cap_only_the_command_fails() {
        let script = "import subprocess, sys, time\n\
                      time.sleep(0.5)\n\
                      r = subprocess.run([sys.executable, '-c', 'bytearray(400 * 2**20)'])\n\
                      print('child', r.returncode)";
        let Some(out) = run_confined(script, "100M") else {
            return;
        };
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success(),
            "{:?}: {stdout} {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
        let code: i32 = stdout
            .trim()
            .strip_prefix("child ")
            .and_then(|c| c.parse().ok())
            .unwrap_or_else(|| panic!("unexpected output: {stdout}"));
        assert_ne!(code, 0, "the command over the cap succeeded");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn agent_runs_in_its_own_scope() {
        let Some(out) = run_confined("print(open('/proc/self/cgroup').read())", "100M") else {
            return;
        };
        let cgroup = String::from_utf8_lossy(&out.stdout);
        assert!(
            cgroup.contains("/ariekei-agent-") && cgroup.trim_end().ends_with(".scope"),
            "{cgroup}"
        );
    }
}
