// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

//! One Ariekei per data directory. Two on the same database would both poll, each would clean
//! up items while the other's agent works on them (it sees only its own agents), and they
//! would overwrite each other's hook settings. The first one locks `ariekei.lock` in the data
//! directory and listens on localhost; a second one asks it to show its window and exits.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::time::Duration;

/// Holding it keeps the data directory to this process.
pub struct Instance {
    _lock: File,
}

/// Takes the data directory for this process (`Some`), calling `show` whenever another
/// Ariekei is started on it; or, if one runs on it already, asks that one to show itself
/// (`None`).
pub fn claim(
    data_dir: &Path,
    show: impl Fn() + Send + 'static,
) -> std::io::Result<Option<Instance>> {
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(data_dir.join("ariekei.lock"))?;
    // Where the first one listens: "<port> <token>" (the lock file itself may not be
    // readable while locked, on Windows).
    let address = data_dir.join("instance");
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            if let Err(e) = ask_to_show(&address) {
                tracing::warn!("could not reach the Ariekei that is running: {e}");
            }
            return Ok(None);
        }
        // E.g. a file system without locks: better two instances than none.
        Err(TryLockError::Error(e)) => {
            tracing::warn!("could not lock {}: {e}", data_dir.display());
            return Ok(Some(Instance { _lock: lock }));
        }
    }
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let token = uuid::Uuid::new_v4().to_string();
    std::fs::write(
        &address,
        format!("{} {token}", listener.local_addr()?.port()),
    )?;
    std::thread::Builder::new()
        .name("instance".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut line = String::new();
                if BufReader::new(stream.take(100))
                    .read_line(&mut line)
                    .is_ok()
                    && line.trim() == token
                {
                    tracing::info!("another Ariekei was started: showing the window");
                    show();
                }
            }
        })?;
    Ok(Some(Instance { _lock: lock }))
}

fn ask_to_show(address: &Path) -> std::io::Result<()> {
    let text = std::fs::read_to_string(address)?;
    let (port, token) = text
        .trim()
        .split_once(' ')
        .ok_or_else(|| std::io::Error::other(format!("{} is garbled", address.display())))?;
    let port: u16 = port.parse().map_err(std::io::Error::other)?;
    let mut stream =
        TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(2))?;
    stream.write_all(format!("{token}\n").as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    #[test]
    fn a_second_instance_on_the_same_data_shows_the_first() {
        let dir = tempfile::tempdir().unwrap();
        let (shown, asked) = channel();
        let first = claim(dir.path(), move || shown.send(()).unwrap()).unwrap();
        assert!(first.is_some());
        let second = claim(dir.path(), || {}).unwrap();
        assert!(second.is_none(), "the data directory is taken");
        asked.recv_timeout(Duration::from_secs(5)).unwrap();

        // Another data directory is another instance's.
        let other = tempfile::tempdir().unwrap();
        assert!(claim(other.path(), || {}).unwrap().is_some());
        // Once the first one is gone, the next one is the first. (A process another test
        // forks right then holds the lock until it execs: give it a moment.)
        drop(first);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while claim(dir.path(), || {}).unwrap().is_none() {
            assert!(std::time::Instant::now() < deadline, "still taken");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
