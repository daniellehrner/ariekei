// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

//! WebKitGTK runs a window's page in a process of its own. When that process ends (it
//! crashed, or went over WebKit's memory limit) the window shows only grey, while Ariekei and
//! its agents go on: the reason is logged and the page loaded again (its terminals reattach).
//! A page that ends again within a minute of that is left as it is, rather than loaded in a
//! loop. WebKit says "crashed" for any end it did not ask for, also a SIGKILL: how the process
//! ended (a signal, or its exit status) is logged too (`pages`).

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

    /// Whether `/proc/<pid>/stat` is that of a running page process of `ariekei`.
    fn is_page(stat: &str, ariekei: u32) -> bool {
        // "pid (comm) state ppid …", where comm may hold spaces and parentheses.
        let (Some(open), Some(close)) = (stat.find('('), stat.rfind(')')) else {
            return false;
        };
        let mut rest = stat[close + 1..].split_whitespace();
        &stat[open + 1..close] == "WebKitWebProces"
            && rest.next().is_some_and(|state| state != "Z")
            && rest.next().and_then(|ppid| ppid.parse().ok()) == Some(ariekei)
    }

    /// Notes the page processes that are new, and forgets those that ended (e.g. with their
    /// window).
    pub fn note() {
        let ariekei = std::process::id();
        let mut pages = PAGES.lock().unwrap();
        pages.retain(|(_, fd)| exit_status(fd) == Ok(None));
        let Ok(dir) = std::fs::read_dir("/proc") else {
            return;
        };
        for entry in dir.flatten() {
            let Some(pid) = entry.file_name().to_str().and_then(|n| n.parse().ok()) else {
                continue;
            };
            if pages.iter().any(|(p, _)| *p == pid)
                || !std::fs::read_to_string(entry.path().join("stat"))
                    .is_ok_and(|stat| is_page(&stat, ariekei))
            {
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
        fn page_processes_are_running_children_named_webkitwebproces() {
            let page = "584620 (WebKitWebProces) S 424861 584620 6012 0 -1 4194560";
            assert!(is_page(page, 424861));
            assert!(!is_page(page, 1));
            assert!(!is_page(&page.replace(" S ", " Z "), 424861));
            assert!(!is_page("424875 (WebKitNetworkPr) S 424861 424875", 424861));
            assert!(!is_page("7 (a (WebKitWebProces) S 424861 7", 424861));
            assert!(!is_page("", 424861));
        }

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
