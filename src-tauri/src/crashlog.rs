// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

//! Why Ariekei ended, in its log (stdout and stderr; the journal when the desktop started it):
//! - each run marks itself in the data directory (`running`) and removes the mark when it
//!   exits cleanly, so the next run says when the previous one did not (killed with SIGKILL,
//!   e.g. a Force Quit, or a crash that left no other trace);
//! - a panic is logged with its backtrace (also one that is caught, e.g. in a hook);
//! - a fatal or termination signal is named (Unix), and so is an `exit()` that no quit asked
//!   for (e.g. GTK losing the display);
//! - quitting says why (`quitting`).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Set once Ariekei quits on purpose: an `exit()` before that is unexpected.
static QUITTING: AtomicBool = AtomicBool::new(false);

const MARK: &str = "running";

/// Installs the panic hook, the signal handlers and the `exit()` hook. First thing in `run`.
pub fn install() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        tracing::error!(
            "panic in thread {}: {info}\n{}",
            thread.name().unwrap_or("<unnamed>"),
            std::backtrace::Backtrace::force_capture()
        );
        default(info);
    }));
    #[cfg(unix)]
    signals::install();
}

/// Marks this run in `data_dir`, after saying if the previous one ended without exiting
/// cleanly. Only for the Ariekei that owns the data directory (`instance`).
pub fn started(data_dir: &Path) {
    let mark = data_dir.join(MARK);
    if let Ok(previous) = std::fs::read_to_string(&mark) {
        tracing::warn!(
            "the previous Ariekei ({}) ended without exiting cleanly: its last lines in this log \
             come before this one; if they name no signal or panic, it was killed with SIGKILL \
             (e.g. a Force Quit)",
            previous.trim()
        );
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let this = format!(
        "pid {}, version {}, started at Unix time {now}",
        std::process::id(),
        env!("CARGO_PKG_VERSION")
    );
    tracing::info!("Ariekei started: {this}");
    if let Err(e) = std::fs::write(&mark, &this) {
        tracing::warn!("could not write {}: {e}", mark.display());
    }
    *DATA_DIR.lock().unwrap() = Some(data_dir.to_owned());
}

static DATA_DIR: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Ariekei quits on purpose (`why`): logged, and the next run will not report it.
pub fn quitting(why: &str) {
    if !QUITTING.swap(true, Ordering::SeqCst) {
        tracing::info!("quitting: {why}");
    }
}

/// The last thing a clean exit does: removes the mark of this run.
pub fn exited() {
    QUITTING.store(true, Ordering::SeqCst);
    if let Some(dir) = DATA_DIR.lock().unwrap().as_ref() {
        let _ = std::fs::remove_file(dir.join(MARK));
    }
    tracing::info!("Ariekei exited cleanly");
}

#[cfg(unix)]
mod signals {
    use super::QUITTING;
    use std::sync::atomic::Ordering;

    /// Writes `text` to stderr: all a signal handler may do safely.
    fn say(text: &[u8]) {
        // SAFETY: write(2) is async-signal-safe; the buffer is valid for its length.
        unsafe {
            libc::write(2, text.as_ptr().cast(), text.len());
        }
    }

    fn name(signal: libc::c_int) -> &'static [u8] {
        match signal {
            libc::SIGSEGV => b"SIGSEGV (invalid memory access)",
            libc::SIGBUS => b"SIGBUS (bad memory access)",
            libc::SIGILL => b"SIGILL (illegal instruction)",
            libc::SIGFPE => b"SIGFPE (arithmetic error)",
            libc::SIGABRT => b"SIGABRT (aborted)",
            libc::SIGTERM => b"SIGTERM (asked to terminate)",
            libc::SIGHUP => b"SIGHUP (hang-up)",
            libc::SIGINT => b"SIGINT (interrupted)",
            libc::SIGQUIT => b"SIGQUIT (quit)",
            _ => b"a signal",
        }
    }

    const FAULTS: [libc::c_int; 5] = [
        libc::SIGSEGV,
        libc::SIGBUS,
        libc::SIGILL,
        libc::SIGFPE,
        libc::SIGABRT,
    ];
    const ENDS: [libc::c_int; 4] = [libc::SIGTERM, libc::SIGHUP, libc::SIGINT, libc::SIGQUIT];

    /// The handlers there were before (Rust's own, for stack overflows): a fault goes on to
    /// them after it is named.
    static mut PREVIOUS: [Option<libc::sigaction>; 32] = [None; 32];

    extern "C" fn on_fault(signal: libc::c_int, info: *mut libc::siginfo_t, _: *mut libc::c_void) {
        say(b"\nAriekei: fatal signal ");
        say(name(signal));
        say(b"\n");
        // SAFETY: sigaction(2) is async-signal-safe; PREVIOUS is written only in `install`,
        // before any handler can run.
        unsafe {
            // A signal that was sent (kill, raise, abort()) ends the process as it would have;
            // a fault, once this returns, runs its instruction again with the previous handler
            // (Rust's names a stack overflow).
            // si_code tells: Linux gives a sent one zero or less (SI_USER, SI_QUEUE, SI_TKILL),
            // macOS 0x10001 and up (SI_USER, …); a fault gets a small positive one there and here.
            let sent = signal == libc::SIGABRT
                || info.is_null()
                || (*info).si_code <= 0
                || (*info).si_code >= 0x10000;
            if sent {
                libc::signal(signal, libc::SIG_DFL);
                libc::raise(signal);
                return;
            }
            match (*std::ptr::addr_of!(PREVIOUS))
                .get(signal as usize)
                .copied()
                .flatten()
            {
                Some(action) => libc::sigaction(signal, &action, std::ptr::null_mut()),
                None => {
                    libc::signal(signal, libc::SIG_DFL);
                    0
                }
            };
        }
    }

    extern "C" fn on_end(signal: libc::c_int) {
        say(b"\nAriekei: ended by ");
        say(name(signal));
        say(b"\n");
        // SA_RESETHAND made the default action current: it ends the process once this returns.
        // SAFETY: raise(3) is async-signal-safe.
        unsafe {
            libc::raise(signal);
        }
    }

    extern "C" fn on_exit() {
        if !QUITTING.load(Ordering::SeqCst) {
            say(b"\nAriekei: exit() called without quitting (e.g. GTK lost the display)\n");
        }
    }

    pub fn install() {
        // SAFETY: plain sigaction(2)/atexit(3) calls with valid arguments, before other
        // threads that could take these signals exist.
        unsafe {
            for signal in FAULTS {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = on_fault as *const () as usize;
                // On the alternate stack Rust sets up, so a stack overflow can be named too.
                action.sa_flags = libc::SA_ONSTACK | libc::SA_SIGINFO;
                libc::sigemptyset(&mut action.sa_mask);
                let mut previous: libc::sigaction = std::mem::zeroed();
                if libc::sigaction(signal, &action, &mut previous) == 0 {
                    (*std::ptr::addr_of_mut!(PREVIOUS))[signal as usize] = Some(previous);
                }
            }
            for signal in ENDS {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = on_end as *const () as usize;
                action.sa_flags = libc::SA_RESETHAND;
                libc::sigemptyset(&mut action.sa_mask);
                libc::sigaction(signal, &action, std::ptr::null_mut());
            }
            libc::atexit(on_exit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_that_did_not_exit_cleanly_is_reported_by_the_next() {
        let dir = tempfile::tempdir().unwrap();
        started(dir.path());
        let mark = std::fs::read_to_string(dir.path().join(MARK)).unwrap();
        assert!(
            mark.starts_with(&format!("pid {},", std::process::id())),
            "{mark}"
        );
        exited();
        assert!(!dir.path().join(MARK).exists());
    }

    /// Runs this test binary's `crashes` test in a child that crashes as `how` says; returns
    /// its stderr and the signal that ended it.
    #[cfg(unix)]
    fn crash(how: &str) -> (String, Option<i32>) {
        use std::os::unix::process::ExitStatusExt;
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "crashlog::tests::crashes",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("ARIEKEI_TEST_CRASH", how)
            .output()
            .unwrap();
        (
            String::from_utf8_lossy(&out.stderr).into_owned(),
            out.status.signal(),
        )
    }

    /// Crashes as `ARIEKEI_TEST_CRASH` says (run by `crash`; does nothing otherwise).
    #[cfg(unix)]
    #[test]
    fn crashes() {
        let Ok(how) = std::env::var("ARIEKEI_TEST_CRASH") else {
            return;
        };
        install();
        #[allow(unconditional_recursion)]
        fn deeper(n: u64) -> u64 {
            std::hint::black_box(deeper(std::hint::black_box(n + 1))) + 1
        }
        match how.as_str() {
            // SAFETY: none; the point is the crash.
            "fault" => unsafe { std::ptr::null_mut::<u8>().write_volatile(1) },
            "overflow" => {
                std::thread::spawn(|| deeper(0)).join().ok();
            }
            "sent" => unsafe {
                libc::raise(libc::SIGSEGV);
            },
            "term" => unsafe {
                libc::raise(libc::SIGTERM);
            },
            _ => {}
        }
        std::thread::sleep(std::time::Duration::from_secs(5));
        panic!("still alive after {how}");
    }

    #[cfg(unix)]
    #[test]
    fn fatal_signals_are_named_and_still_end_it() {
        let (stderr, signal) = crash("fault");
        assert!(stderr.contains("Ariekei: fatal signal SIGSEGV"), "{stderr}");
        assert_eq!(signal, Some(libc::SIGSEGV));
        let (stderr, signal) = crash("sent");
        assert!(stderr.contains("Ariekei: fatal signal SIGSEGV"), "{stderr}");
        assert_eq!(signal, Some(libc::SIGSEGV));
        // Rust's own handler still names a stack overflow.
        let (stderr, signal) = crash("overflow");
        assert!(stderr.contains("has overflowed its stack"), "{stderr}");
        assert_eq!(signal, Some(libc::SIGABRT));
        let (stderr, signal) = crash("term");
        assert!(stderr.contains("Ariekei: ended by SIGTERM"), "{stderr}");
        assert_eq!(signal, Some(libc::SIGTERM));
    }
}
