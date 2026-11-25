// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

//! WebKitGTK runs a window's page in a process of its own. When that process ends (it
//! crashed, or went over WebKit's memory limit) the window shows only grey, while Ariekei and
//! its agents go on: the reason is logged and the page loaded again (its terminals reattach).
//! A page that ends again within a minute of that is left as it is, rather than loaded in a
//! loop. WebKit says "crashed" for any end it did not ask for, also a SIGKILL: how the process
//! ended (a signal, or its exit status) is logged too (`pages`).
//!
//! On Linux, WebKitGTK before 2.54 runs some of its threads in real time, and the kernel kills
//! a process whose real-time thread runs too long: Ariekei sets the limit so that WebKit is told
//! first (`realtime`).

#[cfg(target_os = "linux")]
use std::collections::BTreeSet;
#[cfg(target_os = "linux")]
use std::sync::Mutex;

/// The windows whose page is watched.
#[cfg(target_os = "linux")]
static WATCHED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// Watches the page of `webview` (once per window; called on every page load).
#[cfg(target_os = "linux")]
pub fn watch<R: tauri::Runtime>(webview: &tauri::Webview<R>) {
    std::thread::spawn(pages::note);
    let label = webview.label().to_owned();
    if !WATCHED.lock().unwrap().insert(label.clone()) {
        return;
    }
    let watched = webview.with_webview(move |platform| {
        use std::cell::Cell;
        use std::time::{Duration, Instant};
        use webkit2gtk::WebViewExt;
        let reloaded: Cell<Option<Instant>> = Cell::new(None);
        platform
            .inner()
            .connect_web_process_terminated(move |view, reason| {
                pages::report();
                let again = reloaded
                    .get()
                    .is_some_and(|at| at.elapsed() < Duration::from_secs(60));
                if again {
                    tracing::error!(
                        "the page of window {label} ended again ({reason:?}) within a minute of \
                         loading it again: left as it is; restart Ariekei"
                    );
                    return;
                }
                tracing::error!("the page of window {label} ended ({reason:?}): loading it again");
                reloaded.set(Some(Instant::now()));
                view.reload();
            });
    });
    if let Err(e) = watched {
        tracing::warn!("cannot watch the page of window {}: {e}", webview.label());
    }
}

#[cfg(not(target_os = "linux"))]
pub fn watch<R: tauri::Runtime>(_webview: &tauri::Webview<R>) {}

/// Window `label` is gone: a new one of that name is watched again.
pub fn forget(label: &str) {
    #[cfg(target_os = "linux")]
    WATCHED.lock().unwrap().remove(label);
    #[cfg(not(target_os = "linux"))]
    let _ = label;
}

/// Keeps the kernel from killing Ariekei and its WebKit processes over WebKit's real-time
/// threads (`realtime`). Once logging is set up, before WebKit starts.
pub fn guard_realtime() {
    #[cfg(target_os = "linux")]
    realtime::start();
}

/// The command name (the kernel keeps 15 bytes of it) of the process whose `/proc/<pid>/stat`
/// is `stat`, if it is a running child of `parent`.
#[cfg(target_os = "linux")]
fn child_command(stat: &str, parent: u32) -> Option<&str> {
    // "pid (comm) state ppid …", where comm may hold spaces and parentheses.
    let (open, close) = (stat.find('(')?, stat.rfind(')')?);
    if close < open {
        return None;
    }
    let mut rest = stat[close + 1..].split_whitespace();
    let running = rest.next().is_some_and(|state| state != "Z");
    let ppid: Option<u32> = rest.next().and_then(|ppid| ppid.parse().ok());
    (running && ppid == Some(parent)).then(|| &stat[open + 1..close])
}

/// Ariekei's running child processes whose command name `wanted` takes: their pid and name.
#[cfg(target_os = "linux")]
fn children(wanted: impl Fn(&str) -> bool) -> Vec<(u32, String)> {
    let ariekei = std::process::id();
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    dir.flatten()
        .filter_map(|entry| {
            let pid = entry.file_name().to_str()?.parse().ok()?;
            let stat = std::fs::read_to_string(entry.path().join("stat")).ok()?;
            let command = child_command(&stat, ariekei).filter(|c| wanted(c))?;
            Some((pid, command.to_owned()))
        })
        .collect()
}

/// WebKitGTK before 2.54 runs some threads in real time (SCHED_RR, through rtkit), in Ariekei
/// and in its WebKit processes. A real-time thread that runs for RLIMIT_RTTIME's hard limit
/// (rtkit's 200 ms) without blocking gets its whole process killed with SIGKILL. At the soft
/// limit the kernel sends SIGXCPU first, on which WebKit takes its threads out of real time.
/// WebKit sets the soft limit to 80 % of the hard one, but only when it lowers the hard limit
/// itself. An app that GNOME starts inherits gnome-shell's limits, 200 ms soft and hard, which
/// WebKit leaves as they are: no SIGXCPU, and the kernel kills. So Ariekei sets the soft limit
/// as WebKit would: for itself at the start (its WebKit processes inherit it), and every few
/// seconds again for itself and its WebKit processes, as the kernel raises the soft limit by a
/// second with each SIGXCPU.
#[cfg(target_os = "linux")]
mod realtime {
    use std::time::Duration;

    /// How often the limits are set again.
    const EVERY: Duration = Duration::from_secs(10);

    /// The soft limit (µs) that comes before hard limit `hard`, if soft limit `soft` does not.
    pub(super) fn lowered(soft: u64, hard: u64) -> Option<u64> {
        if hard == libc::RLIM_INFINITY {
            return None;
        }
        let wanted = hard / 5 * 4;
        (soft > wanted).then_some(wanted)
    }

    /// Sets the soft limit of process `pid` (0: Ariekei) as `lowered` says. Returns the limits
    /// it had, if it set them.
    pub(super) fn guard(pid: u32) -> std::io::Result<Option<libc::rlimit>> {
        let mut had = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: prlimit(2) reading into a valid rlimit.
        let read = unsafe {
            libc::prlimit(
                pid as libc::pid_t,
                libc::RLIMIT_RTTIME,
                std::ptr::null(),
                &mut had,
            )
        };
        if read < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let Some(soft) = lowered(had.rlim_cur, had.rlim_max) else {
            return Ok(None);
        };
        let new = libc::rlimit {
            rlim_cur: soft,
            rlim_max: had.rlim_max,
        };
        // SAFETY: prlimit(2) with a valid rlimit; lowering a soft limit needs no privilege.
        let set = unsafe {
            libc::prlimit(
                pid as libc::pid_t,
                libc::RLIMIT_RTTIME,
                &new,
                std::ptr::null_mut(),
            )
        };
        if set < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Some(had))
    }

    /// Sets the limit of process `pid` (0: Ariekei), named `what`, again, and logs it if the
    /// kernel had raised it after a SIGXCPU.
    fn again(pid: u32, what: &str) {
        match guard(pid) {
            Ok(Some(had)) if had.rlim_cur > had.rlim_max => tracing::warn!(
                "a real-time thread of {what} ran {} ms without blocking: WebKit took its \
                 threads out of real time (SIGXCPU) before the kernel would have killed it; its \
                 real-time limit (RLIMIT_RTTIME) is set again",
                had.rlim_max / 5 * 4 / 1000
            ),
            Ok(Some(had)) => tracing::info!(
                "the real-time limit (RLIMIT_RTTIME) of {what} is set again: {} µs, {} µs \
                 before",
                had.rlim_max / 5 * 4,
                had.rlim_cur
            ),
            Ok(None) => {}
            // Gone, e.g.
            Err(e) => tracing::debug!("cannot set the real-time limit of {what}: {e}"),
        }
    }

    pub fn start() {
        match guard(0) {
            Ok(Some(had)) => tracing::info!(
                "real-time limit (RLIMIT_RTTIME) {} µs: its soft limit is set to {} µs (from {} \
                 µs), so that WebKit takes a real-time thread that runs too long out of real \
                 time (SIGXCPU) before the kernel kills Ariekei",
                had.rlim_max,
                had.rlim_max / 5 * 4,
                had.rlim_cur
            ),
            Ok(None) => {}
            Err(e) => tracing::warn!("cannot set the real-time limit (RLIMIT_RTTIME): {e}"),
        }
        let started = std::thread::Builder::new()
            .name("realtime limit".into())
            .spawn(|| {
                loop {
                    std::thread::sleep(EVERY);
                    again(0, "Ariekei");
                    for (pid, command) in super::children(|c| c.starts_with("WebKit")) {
                        again(pid, &format!("{command} (pid {pid})"));
                    }
                }
            });
        if let Err(e) = started {
            tracing::warn!("cannot keep the real-time limit set: {e}");
        }
    }
}

/// How a page process ended. Ariekei is its parent, and WebKit reaps it; a pidfd held for it
/// tells how it ended once reaped (Linux 6.15 or later; before that nothing is logged).
#[cfg(target_os = "linux")]
mod pages {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    /// Ariekei's page processes (WebKitGTK's `WebKitWebProcess`), with a pidfd each.
    static PAGES: Mutex<Vec<(u32, OwnedFd)>> = Mutex::new(Vec::new());

    /// `_IOWR(PIDFS_IOCTL_MAGIC, 11, struct pidfd_info)` (linux/pidfd.h).
    const PIDFD_GET_INFO: libc::c_ulong = 0xC040_FF0B;
    const PIDFD_INFO_EXIT: u64 = 1 << 3;

    /// `struct pidfd_info` of Linux 6.15, 64 bytes.
    #[repr(C)]
    #[derive(Default)]
    struct PidfdInfo {
        mask: u64,
        cgroupid: u64,
        ids: [u32; 11],
        exit_code: i32,
    }

    /// The wait status of the process of `pidfd` once it ended and was reaped; `Err` when the
    /// kernel cannot tell.
    fn exit_status(pidfd: &OwnedFd) -> Result<Option<i32>, ()> {
        let mut info = PidfdInfo {
            mask: PIDFD_INFO_EXIT,
            ..Default::default()
        };
        // SAFETY: PIDFD_GET_INFO on a pidfd, with a struct of the size the request names.
        if unsafe { libc::ioctl(pidfd.as_raw_fd(), PIDFD_GET_INFO as _, &mut info) } < 0 {
            return Err(());
        }
        Ok((info.mask & PIDFD_INFO_EXIT != 0).then_some(info.exit_code))
    }

    /// A pidfd for process `pid`, closed on exec.
    fn pidfd(pid: u32) -> Option<OwnedFd> {
        // SAFETY: pidfd_open(2) with no flags.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0) };
        // SAFETY: a new descriptor that nothing else owns.
        (fd >= 0).then(|| unsafe { OwnedFd::from_raw_fd(fd as libc::c_int) })
    }

    /// The command name of WebKitGTK's page processes (`WebKitWebProcess`, cut to 15 bytes).
    const PAGE: &str = "WebKitWebProces";

    /// Notes the page processes that are new, and forgets those that ended (e.g. with their
    /// window).
    pub fn note() {
        let mut pages = PAGES.lock().unwrap();
        pages.retain(|(_, fd)| exit_status(fd) == Ok(None));
        for (pid, _) in super::children(|command| command == PAGE) {
            if pages.iter().any(|(p, _)| *p == pid) {
                continue;
            }
            if let Some(fd) = pidfd(pid).filter(|fd| exit_status(fd) == Ok(None)) {
                pages.push((pid, fd));
            }
        }
    }

    /// A page ended: logs how its process ended, once WebKit has reaped it. Takes every page
    /// process, as which one was the window's is not known, and keeps on those still running.
    pub fn report() {
        let mut held = std::mem::take(&mut *PAGES.lock().unwrap());
        if held.is_empty() {
            return;
        }
        std::thread::spawn(move || {
            let until = Instant::now() + Duration::from_secs(5);
            loop {
                held.retain(|(pid, fd)| match exit_status(fd) {
                    Ok(Some(status)) => {
                        let (how, _) = crate::crashlog::ended(status);
                        tracing::error!("the page process (pid {pid}) {how}");
                        false
                    }
                    Ok(None) => true,
                    Err(()) => false,
                });
                if held.is_empty() || Instant::now() >= until {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            let mut pages = PAGES.lock().unwrap();
            for (pid, fd) in held {
                if !pages.iter().any(|(p, _)| *p == pid) {
                    pages.push((pid, fd));
                }
            }
        });
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_reaped_process_says_how_it_ended() {
            let mut child = std::process::Command::new("sleep")
                .arg("30")
                .spawn()
                .unwrap();
            let fd = pidfd(child.id()).unwrap();
            let running = exit_status(&fd);
            child.kill().unwrap();
            child.wait().unwrap();
            if running.is_err() {
                return; // before Linux 6.15
            }
            assert_eq!(running, Ok(None));
            let status = exit_status(&fd).unwrap().unwrap();
            assert!(
                crate::crashlog::ended(status)
                    .0
                    .starts_with("was ended by signal 9 (SIGKILL: ")
            );
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn a_child_is_named_by_its_stat_when_it_runs() {
        let page = "584620 (WebKitWebProces) S 424861 584620 6012 0 -1 4194560";
        assert_eq!(child_command(page, 424861), Some("WebKitWebProces"));
        assert_eq!(child_command(page, 1), None);
        assert_eq!(child_command(&page.replace(" S ", " Z "), 424861), None);
        assert_eq!(
            child_command("7 (a (b) c) S 424861 7", 424861),
            Some("a (b) c")
        );
        assert_eq!(child_command(") 7 ( S 424861", 424861), None);
        assert_eq!(child_command("", 424861), None);
    }

    #[test]
    fn the_soft_real_time_limit_comes_before_the_hard_one() {
        let lowered = realtime::lowered;
        // Inherited from gnome-shell.
        assert_eq!(lowered(200_000, 200_000), Some(160_000));
        // As WebKit sets it.
        assert_eq!(lowered(160_000, 200_000), None);
        assert_eq!(lowered(100_000, 200_000), None);
        // Raised by the kernel after a SIGXCPU.
        assert_eq!(lowered(1_160_000, 200_000), Some(160_000));
        // No limit: WebKit sets one itself.
        assert_eq!(lowered(libc::RLIM_INFINITY, libc::RLIM_INFINITY), None);
    }

    #[test]
    fn a_process_gets_its_soft_real_time_limit_lowered() {
        fn limits(pid: u32) -> libc::rlimit {
            let mut now = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            // SAFETY: prlimit(2) reading into a valid rlimit.
            let read = unsafe {
                libc::prlimit(
                    pid as libc::pid_t,
                    libc::RLIMIT_RTTIME,
                    std::ptr::null(),
                    &mut now,
                )
            };
            assert_eq!(read, 0, "{}", std::io::Error::last_os_error());
            now
        }
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = child.id();
        // As an app GNOME starts has them (lowering a hard limit needs no privilege).
        let hard = limits(pid).rlim_max.min(200_000);
        let same = libc::rlimit {
            rlim_cur: hard,
            rlim_max: hard,
        };
        // SAFETY: prlimit(2) with a valid rlimit.
        let set = unsafe {
            libc::prlimit(
                pid as libc::pid_t,
                libc::RLIMIT_RTTIME,
                &same,
                std::ptr::null_mut(),
            )
        };
        let set = (set == 0)
            .then_some(())
            .ok_or(std::io::Error::last_os_error());
        let guarded = set.and_then(|()| realtime::guard(pid));
        let after = limits(pid);
        let again = realtime::guard(pid);
        child.kill().unwrap();
        child.wait().unwrap();

        let had = guarded.unwrap().expect("lowered");
        assert_eq!((had.rlim_cur, had.rlim_max), (hard, hard));
        assert_eq!((after.rlim_cur, after.rlim_max), (hard / 5 * 4, hard));
        assert!(again.unwrap().is_none());
    }
}
