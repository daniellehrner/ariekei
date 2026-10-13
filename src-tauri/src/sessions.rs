//! Spike-only persistence of agent session ids per worktree (SQLite in M1).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

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

    fn read(&self) -> BTreeMap<String, String> {
        std::fs::read(&self.path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn get(&self, key: &str) -> Option<String> {
        let _guard = self.lock.lock().unwrap();
        self.read().remove(key)
    }

    pub fn put(&self, key: &str, session_id: &str) -> Result<(), String> {
        let _guard = self.lock.lock().unwrap();
        let mut all = self.read();
        all.insert(key.to_owned(), session_id.to_owned());
        let json = serde_json::to_vec_pretty(&all).map_err(|e| e.to_string())?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }
}
