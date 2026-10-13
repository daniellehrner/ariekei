//! Spike-only persistence of each worktree's Claude session (SQLite in M1).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub session_id: String,
    /// Set once a prompt was submitted; Claude Code only saves a transcript after that.
    #[serde(default)]
    pub started: bool,
    #[serde(default)]
    pub transcript_path: Option<PathBuf>,
}

impl Session {
    pub fn new(session_id: String) -> Self {
        Self {
            session_id,
            started: false,
            transcript_path: None,
        }
    }
}

pub struct SessionStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl SessionStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
        }
    }

    fn read(&self) -> BTreeMap<String, Session> {
        std::fs::read(&self.path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn write(&self, all: &BTreeMap<String, Session>) -> Result<(), String> {
        let json = serde_json::to_vec_pretty(all).map_err(|e| e.to_string())?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }

    pub fn get(&self, key: &str) -> Option<Session> {
        let _guard = self.lock.lock().unwrap();
        self.read().remove(key)
    }

    pub fn put(&self, key: &str, session: &Session) -> Result<(), String> {
        let _guard = self.lock.lock().unwrap();
        let mut all = self.read();
        all.insert(key.to_owned(), session.clone());
        self.write(&all)
    }

    /// Records that the session has a conversation (and where its transcript is).
    pub fn mark_started(&self, key: &str, transcript_path: Option<&Path>) {
        let _guard = self.lock.lock().unwrap();
        let mut all = self.read();
        let Some(session) = all.get_mut(key) else {
            return;
        };
        if session.started
            && transcript_path.is_none_or(|p| session.transcript_path.as_deref() == Some(p))
        {
            return;
        }
        session.started = true;
        if let Some(p) = transcript_path {
            session.transcript_path = Some(p.to_owned());
        }
        if let Err(e) = self.write(&all) {
            tracing::warn!("failed to save session state: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_started_persists_flag_and_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(dir.path().join("sessions.json"));
        store.put("/w", &Session::new("id".into())).unwrap();
        store.mark_started("/w", Some(Path::new("/t.jsonl")));

        let reopened = SessionStore::new(dir.path().join("sessions.json"));
        let s = reopened.get("/w").unwrap();
        assert!(s.started);
        assert_eq!(s.transcript_path.as_deref(), Some(Path::new("/t.jsonl")));
        store.mark_started("/unknown", None); // no entry: ignored
        assert!(reopened.get("/unknown").is_none());
    }
}
