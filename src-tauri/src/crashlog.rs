// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

//! Why Ariekei ended, in its log (stdout and stderr; the journal when the desktop started it):
//! - each run marks itself in the data directory (`running`) and removes the mark when it
//!   exits cleanly, so the next run says when the previous one did not (killed with SIGKILL,
//!   e.g. a Force Quit, or a crash that left no other trace);
//! - a panic is logged with its backtrace (also one that is caught, e.g. in a hook);
//! - a fatal or termination signal is named (Unix), and so is an `exit()` that no quit asked
//!   for (e.g. GTK losing the display);
//! - quitting says why (`quitting`);
//! - on Linux a watcher process says how Ariekei ended, also what no handler sees: SIGKILL, a
//!   signal Ariekei does not catch, an `_exit()` (`watch`).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Set once Ariekei quits on purpose: an `exit()` before that is unexpected.
static QUITTING: AtomicBool = AtomicBool::new(false);

const MARK: &str = "running";

/// Linux: Ariekei goes on in a child process, and this one waits for it and logs how it ended.
/// First thing in `run`, while the process has one thread. Not in the e2e build, whose driver
/// keeps track of the process it started.
pub fn watch() {
    #[cfg(all(target_os = "linux", not(feature = "e2e")))]
    watcher::run();
}

/// Installs the panic hook, the signal handlers and the `exit()` hook. After `watch`.
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
             come before this one, on Linux with its watcher's on how it ended; elsewhere, if \
             they name no signal or panic, it was killed with SIGKILL (e.g. a Force Quit)",
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

    /// Linux: who sent the signal, if a process did: ", sent by pid 123 (its command)".
    ///
    /// SAFETY: `info` is the kernel's, or null; only async-signal-safe calls (write, open,
    /// read, close).
    #[cfg(target_os = "linux")]
    unsafe fn say_sender(info: *const libc::siginfo_t) {
        /// `n` in decimal, at the end of `buf`.
        fn decimal(mut n: u32, buf: &mut [u8; 10]) -> &[u8] {
            let mut at = buf.len();
            loop {
                at -= 1;
                buf[at] = b'0' + (n % 10) as u8;
                n /= 10;
                if n == 0 {
                    return &buf[at..];
                }
            }
        }
        // SI_USER, SI_QUEUE, SI_TKILL…: zero or less.
        if info.is_null() || unsafe { (*info).si_code } > 0 {
            return;
        }
        let pid = unsafe { (*info).si_pid() } as u32;
        let mut digits = [0; 10];
        let digits = decimal(pid, &mut digits);
        say(b", sent by pid ");
        say(digits);
        let mut path = [0u8; 32];
        let mut len = 0;
        for part in [b"/proc/".as_slice(), digits, b"/comm\0"] {
            path[len..len + part.len()].copy_from_slice(part);
            len += part.len();
        }
        unsafe {
            let fd = libc::open(path.as_ptr().cast(), libc::O_RDONLY | libc::O_CLOEXEC);
            if fd < 0 {
                return;
            }
            let mut comm = [0u8; 16];
            let read = libc::read(fd, comm.as_mut_ptr().cast(), comm.len());
            libc::close(fd);
            if read > 0 {
                let comm = &comm[..read as usize];
                say(b" (");
                say(comm.strip_suffix(b"\n").unwrap_or(comm));
                say(b")");
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    unsafe fn say_sender(_: *const libc::siginfo_t) {}

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
        // Not for an abort, which may follow a stack overflow (see below).
        if signal != libc::SIGABRT {
            // SAFETY: `info` is the kernel's, or null.
            unsafe { say_sender(info) };
        }
        say(b"\n");
        // SAFETY: sigaction(2) is async-signal-safe; PREVIOUS is written only in `install`,
        // before any handler can run.
        unsafe {
            // A signal that was sent (kill, raise, abort()) ends the process as it would have;
            // a fault, once this returns, runs its instruction again with the previous handler
            // (Rust's names a stack overflow).
            // si_code tells: Linux gives a sent one zero or less (SI_USER, SI_QUEUE, SI_TKILL),
            // macOS 0x10001 and up (SI_USER, …); a fault gets a small positive one there and here.
            // Not always on macOS: a SIGSEGV the process raised itself gets one like a fault's,
            // so it goes on to Rust's handler and the process lives on, as without this one.
            // abort() raises its signal again once the handler returns. With as little as that
            // here: an abort after a stack overflow runs on what is left of the signal stack.
            if signal == libc::SIGABRT {
                libc::signal(signal, libc::SIG_DFL);
                libc::raise(signal);
                return;
            }
            let sent = info.is_null() || (*info).si_code <= 0 || (*info).si_code >= 0x10000;
            if sent {
                die(signal);
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

    extern "C" fn on_end(signal: libc::c_int, info: *mut libc::siginfo_t, _: *mut libc::c_void) {
        say(b"\nAriekei: ended by ");
        say(name(signal));
        // SAFETY: `info` is the kernel's, or null.
        unsafe { say_sender(info) };
        say(b"\n");
        // SAFETY: see `die`.
        unsafe { die(signal) }
    }

    /// Ends the process with `signal`'s default action, as it would have without Ariekei's
    /// handler: unblocked (a handler runs with its signal blocked) and raised again; should that
    /// not end it (macOS, raised in its own handler), sent to the process, which another thread
    /// takes; and should it still be alive a second later, exit status 128 + signal.
    ///
    /// SAFETY: only async-signal-safe calls (signal, sigprocmask, raise, kill, nanosleep,
    /// _exit).
    unsafe fn die(signal: libc::c_int) -> ! {
        unsafe {
            libc::signal(signal, libc::SIG_DFL);
            let mut set: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut set);
            libc::sigaddset(&mut set, signal);
            libc::sigprocmask(libc::SIG_UNBLOCK, &set, std::ptr::null_mut());
            // To this thread: delivered before raise returns, now that it is unblocked.
            libc::raise(signal);
            libc::kill(libc::getpid(), signal);
            // The thread that takes it may be a moment.
            let wait = libc::timespec {
                tv_sec: 0,
                tv_nsec: 100_000_000,
            };
            for _ in 0..10 {
                libc::nanosleep(&wait, std::ptr::null_mut());
            }
            libc::_exit(128 + signal)
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
                action.sa_flags = libc::SA_SIGINFO;
                libc::sigemptyset(&mut action.sa_mask);
                libc::sigaction(signal, &action, std::ptr::null_mut());
            }
            libc::atexit(on_exit);
        }
    }
}

/// The process that starts Ariekei waits for it and logs how it ended: its exit status, or the
/// signal that ended it. SIGKILL (the kernel out of memory, systemd-oomd, a Force Quit, a
/// `kill -9`), a signal Ariekei does not catch and an `_exit()` leave no other trace. It passes
/// on the signals that ask Ariekei to end, and its command is `ariekei-watch`.
#[cfg(all(target_os = "linux", any(test, not(feature = "e2e"))))]
mod watcher {
    use std::sync::atomic::{AtomicI32, Ordering};

    static ARIEKEI: AtomicI32 = AtomicI32::new(0);

    extern "C" fn pass_on(signal: libc::c_int) {
        // SAFETY: kill(2) is async-signal-safe.
        unsafe {
            libc::kill(ARIEKEI.load(Ordering::SeqCst), signal);
        }
    }

    /// Returns in the child, which goes on as Ariekei (also when fork fails: unwatched); the
    /// watcher ends with it.
    pub fn run() {
        // SAFETY: fork(2) with one thread (`watch`); the child makes only async-signal-safe
        // calls before it returns.
        let (watcher, ariekei) = unsafe { (libc::getpid(), libc::fork()) };
        if ariekei <= 0 {
            if ariekei == 0 {
                // SAFETY: prctl, getppid and raise with valid arguments.
                unsafe {
                    // Ariekei ends with its watcher, e.g. a `kill -9` of the watcher.
                    libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                    if libc::getppid() != watcher {
                        libc::raise(libc::SIGTERM);
                    }
                }
            }
            return;
        }
        ARIEKEI.store(ariekei, Ordering::SeqCst);
        // SAFETY: prctl and signal with valid arguments; `pass_on` is async-signal-safe.
        unsafe {
            libc::prctl(libc::PR_SET_NAME, c"ariekei-watch".as_ptr());
            for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP, libc::SIGQUIT] {
                libc::signal(signal, pass_on as *const () as libc::sighandler_t);
            }
        }
        let mut status = 0;
        // SAFETY: waitpid on our child with a valid status pointer.
        while unsafe { libc::waitpid(ariekei, &mut status, 0) } < 0 {
            if std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                eprintln!("Ariekei watcher: lost Ariekei (pid {ariekei})");
                std::process::exit(1);
            }
        }
        let (how, code) = ended(status);
        eprintln!("Ariekei watcher: Ariekei (pid {ariekei}) {how}");
        std::process::exit(code)
    }

    /// How a process with wait status `status` ended, and the exit status that says it.
    fn ended(status: libc::c_int) -> (String, i32) {
        if libc::WIFSIGNALED(status) {
            let signal = libc::WTERMSIG(status);
            let what = if signal == libc::SIGKILL {
                "SIGKILL: the kernel out of memory (journalctl -k), systemd-oomd, a Force Quit \
                 or a kill -9"
                    .to_owned()
            } else {
                // SAFETY: strsignal returns a valid C string; one thread here.
                unsafe { std::ffi::CStr::from_ptr(libc::strsignal(signal)) }
                    .to_string_lossy()
                    .into_owned()
            };
            (
                format!("was ended by signal {signal} ({what})"),
                128 + signal,
            )
        } else {
            let code = libc::WEXITSTATUS(status);
            (format!("exited with status {code}"), code)
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

    /// Runs this test binary's `test` in a child, with `env` set to `how`; returns its pid,
    /// its stderr and how it ended.
    #[cfg(unix)]
    fn run_child(test: &str, env: &str, how: &str) -> (u32, String, std::process::ExitStatus) {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture", "--test-threads=1"])
            .env(env, how)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let pid = child.id();
        let out = child.wait_with_output().unwrap();
        // With how it ended, for the failure message.
        (
            pid,
            format!("{}\n[{}]", String::from_utf8_lossy(&out.stderr), out.status),
            out.status,
        )
    }

    /// Runs the `crashes` test in a child that crashes as `how` says; returns its pid, its
    /// stderr and the signal that ended it.
    #[cfg(unix)]
    fn crash(how: &str) -> (u32, String, Option<i32>) {
        use std::os::unix::process::ExitStatusExt;
        let (pid, stderr, status) =
            run_child("crashlog::tests::crashes", "ARIEKEI_TEST_CRASH", how);
        (pid, stderr, status.signal())
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
        // Each case, then all that went wrong.
        let mut cases = vec![
            // A fault names no sender.
            (
                "fault",
                "Ariekei: fatal signal SIGSEGV (invalid memory access)\n",
                libc::SIGSEGV,
            ),
            // Rust's own handler still names a stack overflow.
            ("overflow", "has overflowed its stack", libc::SIGABRT),
            ("term", "Ariekei: ended by SIGTERM", libc::SIGTERM),
        ];
        // macOS cannot tell this one from a fault (see `on_fault`).
        if cfg!(target_os = "linux") {
            cases.push(("sent", "Ariekei: fatal signal SIGSEGV", libc::SIGSEGV));
        }
        let failed: Vec<String> = cases
            .into_iter()
            .filter_map(|(how, says, ends_by)| {
                let (pid, stderr, signal) = crash(how);
                // Linux names who sent it: here the child itself.
                let sender = format!(", sent by pid {pid} (");
                let named = !matches!(how, "term" | "sent")
                    || !cfg!(target_os = "linux")
                    || stderr.contains(&sender);
                (!stderr.contains(says) || !named || signal != Some(ends_by))
                    .then(|| format!("{how}: ended by {signal:?}, not {ends_by}:\n{stderr}"))
            })
            .collect();
        assert!(failed.is_empty(), "{}", failed.join("\n\n"));
    }

    /// Ends as `ARIEKEI_TEST_WATCHED` says, watched (run by `watcher_says_how_it_ended`; does
    /// nothing otherwise).
    #[cfg(target_os = "linux")]
    #[test]
    fn watched() {
        let Ok(how) = std::env::var("ARIEKEI_TEST_WATCHED") else {
            return;
        };
        super::watcher::run();
        // SAFETY: none needed; what no handler sees.
        unsafe {
            match how.as_str() {
                "exit" => libc::_exit(3),
                _ => libc::raise(libc::SIGKILL),
            };
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_watcher_says_how_ariekei_ended() {
        let failed: Vec<String> = [
            ("exit", "exited with status 3", 3),
            ("kill", "was ended by signal 9 (SIGKILL: ", 128 + 9),
        ]
        .into_iter()
        .filter_map(|(how, says, code)| {
            let (_, stderr, status) =
                run_child("crashlog::tests::watched", "ARIEKEI_TEST_WATCHED", how);
            (!stderr.contains(says) || status.code() != Some(code))
                .then(|| format!("{how}: not {code}, {says}:\n{stderr}"))
        })
        .collect();
        assert!(failed.is_empty(), "{}", failed.join("\n\n"));
    }
}
