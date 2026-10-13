//! Persistent app state in SQLite: connected repos and their work items.
//!
//! Migrations are ordered SQL scripts; `PRAGMA user_version` records how many ran.

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const MIGRATIONS: &[&str] = &[
    r#"
CREATE TABLE repos (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    owner TEXT NOT NULL,
    name TEXT NOT NULL,
    -- The git remote that points at owner/name (PR bases live there).
    base_remote TEXT NOT NULL,
    viewer TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE work_items (
    id INTEGER PRIMARY KEY,
    repo_id INTEGER NOT NULL REFERENCES repos(id) ON DELETE CASCADE,
    number INTEGER NOT NULL,
    -- Latest GitHub state as JSON (github::PrSummary); refreshed by the poller.
    snapshot TEXT NOT NULL,
    open INTEGER NOT NULL DEFAULT 1,
    worktree_path TEXT,
    branch TEXT,
    session_id TEXT,
    session_started INTEGER NOT NULL DEFAULT 0,
    transcript_path TEXT,
    reviewed_sha TEXT,
    -- Newest GitHub activity (RFC 3339) the user has seen in Ariekei.
    last_seen TEXT,
    -- View state as JSON, owned by the UI.
    ui_state TEXT,
    UNIQUE (repo_id, number)
);
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
"#,
    r#"
-- Why the item is in the Inbox: 'author' (the viewer's PR), 'requested' (review requested
-- from the viewer) or 'other'. Recomputed on every poll.
ALTER TABLE work_items ADD COLUMN role TEXT NOT NULL DEFAULT 'author';
-- The user opened it, so it stays listed (and polled) even when no query finds it.
ALTER TABLE work_items ADD COLUMN tracked INTEGER NOT NULL DEFAULT 0;
"#,
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Repo {
    pub id: i64,
    pub path: PathBuf,
    pub owner: String,
    pub name: String,
    pub base_remote: String,
    pub viewer: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkItem {
    pub id: i64,
    pub repo_id: i64,
    pub number: i64,
    pub snapshot: serde_json::Value,
    pub open: bool,
    pub worktree_path: Option<PathBuf>,
    pub branch: Option<String>,
    pub session_id: Option<String>,
    pub session_started: bool,
    pub transcript_path: Option<PathBuf>,
    pub reviewed_sha: Option<String>,
    pub last_seen: Option<String>,
    pub ui_state: Option<serde_json::Value>,
    pub role: String,
    pub tracked: bool,
}

/// One PR as found by a poll.
#[derive(Debug, Clone)]
pub struct PolledPr {
    pub number: i64,
    pub role: &'static str,
    pub open: bool,
    pub snapshot: serde_json::Value,
}

pub struct Store {
    conn: Mutex<Connection>,
}

fn err(e: rusqlite::Error) -> String {
    e.to_string()
}

const ITEM_COLUMNS: &str = "id, repo_id, number, snapshot, open, worktree_path, branch, session_id, \
     session_started, transcript_path, reviewed_sha, last_seen, ui_state, role, tracked";

fn item_from_row(r: &rusqlite::Row) -> rusqlite::Result<WorkItem> {
    let json = |s: Option<String>| s.and_then(|s| serde_json::from_str(&s).ok());
    Ok(WorkItem {
        id: r.get(0)?,
        repo_id: r.get(1)?,
        number: r.get(2)?,
        snapshot: json(r.get(3)?).unwrap_or_default(),
        open: r.get(4)?,
        worktree_path: r.get::<_, Option<String>>(5)?.map(PathBuf::from),
        branch: r.get(6)?,
        session_id: r.get(7)?,
        session_started: r.get(8)?,
        transcript_path: r.get::<_, Option<String>>(9)?.map(PathBuf::from),
        reviewed_sha: r.get(10)?,
        last_seen: r.get(11)?,
        ui_state: json(r.get(12)?),
        role: r.get(13)?,
        tracked: r.get(14)?,
    })
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, String> {
        Self::init(Connection::open(path).map_err(err)?)
    }

    #[cfg(test)]
    pub fn in_memory() -> Self {
        Self::init(Connection::open_in_memory().unwrap()).unwrap()
    }

    fn init(mut conn: Connection) -> Result<Self, String> {
        conn.pragma_update(None, "foreign_keys", true)
            .map_err(err)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(err)?;
        let done: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(err)?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(done as usize) {
            let tx = conn.transaction().map_err(err)?;
            tx.execute_batch(sql).map_err(err)?;
            tx.pragma_update(None, "user_version", i as i64 + 1)
                .map_err(err)?;
            tx.commit().map_err(err)?;
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    // --- repos and settings ---

    pub fn add_repo(
        &self,
        path: &Path,
        owner: &str,
        name: &str,
        base_remote: &str,
        viewer: &str,
    ) -> Result<Repo, String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO repos (path, owner, name, base_remote, viewer) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (path) DO UPDATE SET owner = ?2, name = ?3, base_remote = ?4, viewer = ?5",
            params![path_str(path), owner, name, base_remote, viewer],
        )
        .map_err(err)?;
        let id = conn
            .query_row(
                "SELECT id FROM repos WHERE path = ?1",
                [path_str(path)],
                |r| r.get(0),
            )
            .map_err(err)?;
        Ok(Repo {
            id,
            path: path.to_owned(),
            owner: owner.into(),
            name: name.into(),
            base_remote: base_remote.into(),
            viewer: viewer.into(),
        })
    }

    pub fn repo(&self, id: i64) -> Result<Option<Repo>, String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, path, owner, name, base_remote, viewer FROM repos WHERE id = ?1",
            [id],
            |r| {
                Ok(Repo {
                    id: r.get(0)?,
                    path: PathBuf::from(r.get::<_, String>(1)?),
                    owner: r.get(2)?,
                    name: r.get(3)?,
                    base_remote: r.get(4)?,
                    viewer: r.get(5)?,
                })
            },
        )
        .optional()
        .map_err(err)
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>, String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()
        .map_err(err)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = ?2",
            [key, value],
        )
        .map(|_| ())
        .map_err(err)
    }

    /// The repo the Inbox shows (M1 has one).
    pub fn active_repo(&self) -> Result<Option<Repo>, String> {
        match self.setting("active_repo")?.and_then(|v| v.parse().ok()) {
            Some(id) => self.repo(id),
            None => Ok(None),
        }
    }

    // --- work items ---

    /// Records the current GitHub state of the repo's open PRs. Items that are no longer
    /// open are kept (with `open = false`) until cleanup, so no local state is lost.
    /// Records what a poll found. Items it did not find are kept (with `open = false`)
    /// until cleanup, so no local state is lost.
    pub fn sync_prs(&self, repo_id: i64, prs: &[PolledPr]) -> Result<(), String> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction().map_err(err)?;
        tx.execute(
            "UPDATE work_items SET open = 0 WHERE repo_id = ?1",
            [repo_id],
        )
        .map_err(err)?;
        for pr in prs {
            tx.execute(
                "INSERT INTO work_items (repo_id, number, snapshot, open, role) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (repo_id, number) DO UPDATE SET snapshot = ?3, open = ?4, role = ?5",
                params![repo_id, pr.number, pr.snapshot.to_string(), pr.open, pr.role],
            )
            .map_err(err)?;
        }
        tx.commit().map_err(err)
    }

    /// Adds a PR the user picked (e.g. from search) or marks an existing one as tracked.
    pub fn track(&self, repo_id: i64, pr: &PolledPr) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO work_items (repo_id, number, snapshot, open, role, tracked, last_seen)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, '')
             ON CONFLICT (repo_id, number) DO UPDATE SET snapshot = ?3, open = ?4, tracked = 1",
            params![
                repo_id,
                pr.number,
                pr.snapshot.to_string(),
                pr.open,
                pr.role
            ],
        )
        .map_err(err)?;
        conn.query_row(
            "SELECT id FROM work_items WHERE repo_id = ?1 AND number = ?2",
            params![repo_id, pr.number],
            |r| r.get(0),
        )
        .map_err(err)
    }

    pub fn set_tracked(&self, id: i64) -> Result<(), String> {
        self.update(id, "tracked = ?2", true)
    }

    pub fn tracked_numbers(&self, repo_id: i64) -> Result<Vec<i64>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT number FROM work_items WHERE repo_id = ?1 AND tracked = 1")
            .map_err(err)?;
        let rows = stmt.query_map([repo_id], |r| r.get(0)).map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }

    pub fn items(&self, repo_id: i64) -> Result<Vec<WorkItem>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {ITEM_COLUMNS} FROM work_items WHERE repo_id = ?1 ORDER BY number DESC"
            ))
            .map_err(err)?;
        let rows = stmt.query_map([repo_id], item_from_row).map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }

    pub fn item(&self, id: i64) -> Result<WorkItem, String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            &format!("SELECT {ITEM_COLUMNS} FROM work_items WHERE id = ?1"),
            [id],
            item_from_row,
        )
        .map_err(err)
    }

    fn update(&self, id: i64, set: &str, value: impl rusqlite::ToSql) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            &format!("UPDATE work_items SET {set} WHERE id = ?1"),
            params![id, value],
        )
        .map(|_| ())
        .map_err(err)
    }

    pub fn set_worktree(
        &self,
        id: i64,
        path: Option<&Path>,
        branch: Option<&str>,
    ) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE work_items SET worktree_path = ?2, branch = ?3 WHERE id = ?1",
            params![id, path.map(path_str), branch],
        )
        .map(|_| ())
        .map_err(err)
    }

    /// Sets a new session id; the conversation starts unprompted.
    pub fn set_session(&self, id: i64, session_id: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE work_items SET session_id = ?2, session_started = 0, transcript_path = NULL WHERE id = ?1",
            params![id, session_id],
        )
        .map(|_| ())
        .map_err(err)
    }

    /// Records that the session has a conversation (and where its transcript is).
    pub fn mark_session_started(
        &self,
        id: i64,
        transcript_path: Option<&Path>,
    ) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE work_items SET session_started = 1, transcript_path = COALESCE(?2, transcript_path) WHERE id = ?1",
            params![id, transcript_path.map(path_str)],
        )
        .map(|_| ())
        .map_err(err)
    }

    pub fn set_reviewed(&self, id: i64, sha: &str) -> Result<(), String> {
        self.update(id, "reviewed_sha = ?2", sha)
    }

    pub fn set_last_seen(&self, id: i64, at: &str) -> Result<(), String> {
        self.update(id, "last_seen = ?2", at)
    }

    pub fn set_ui_state(&self, id: i64, state: &serde_json::Value) -> Result<(), String> {
        self.update(id, "ui_state = ?2", state.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn polled(number: i64, snapshot: serde_json::Value) -> PolledPr {
        PolledPr {
            number,
            role: "author",
            open: true,
            snapshot,
        }
    }

    fn store_with_repo() -> (Store, Repo) {
        let store = Store::in_memory();
        let repo = store
            .add_repo(Path::new("/src/besu"), "besu-eth", "besu", "origin", "me")
            .unwrap();
        (store, repo)
    }

    #[test]
    fn migrations_run_once_and_are_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.db");
        Store::open(&path).unwrap();
        let again = Store::open(&path).unwrap();
        let version: i64 = again
            .conn
            .lock()
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version as usize, MIGRATIONS.len());
    }

    #[test]
    fn active_repo_round_trip_and_reconnect_keeps_id() {
        let (store, repo) = store_with_repo();
        assert_eq!(store.active_repo().unwrap(), None);
        store
            .set_setting("active_repo", &repo.id.to_string())
            .unwrap();
        assert_eq!(store.active_repo().unwrap(), Some(repo.clone()));
        let again = store
            .add_repo(Path::new("/src/besu"), "besu-eth", "besu", "upstream", "me")
            .unwrap();
        assert_eq!(
            (again.id, again.base_remote.as_str()),
            (repo.id, "upstream")
        );
    }

    #[test]
    fn sync_keeps_local_state_and_marks_vanished_prs_closed() {
        let (store, repo) = store_with_repo();
        store
            .sync_prs(
                repo.id,
                &[
                    polled(1, json!({"title": "a"})),
                    polled(2, json!({"title": "b"})),
                ],
            )
            .unwrap();
        let one = store
            .items(repo.id)
            .unwrap()
            .into_iter()
            .find(|i| i.number == 1)
            .unwrap();
        store
            .set_worktree(one.id, Some(Path::new("/src/besu_pr1")), Some("feat"))
            .unwrap();
        store.set_reviewed(one.id, "abc").unwrap();

        store
            .sync_prs(repo.id, &[polled(1, json!({"title": "a2"}))])
            .unwrap();
        let items = store.items(repo.id).unwrap();
        let one = items.iter().find(|i| i.number == 1).unwrap();
        let two = items.iter().find(|i| i.number == 2).unwrap();
        assert!(one.open && !two.open);
        assert_eq!(one.snapshot["title"], "a2");
        assert_eq!(
            one.worktree_path.as_deref(),
            Some(Path::new("/src/besu_pr1"))
        );
        assert_eq!(one.reviewed_sha.as_deref(), Some("abc"));
    }

    #[test]
    fn tracking_keeps_a_pr_and_roles_follow_the_poll() {
        let (store, repo) = store_with_repo();
        let requested = PolledPr {
            role: "requested",
            ..polled(5, json!({"title": "theirs"}))
        };
        store.sync_prs(repo.id, &[requested]).unwrap();
        assert_eq!(store.items(repo.id).unwrap()[0].role, "requested");
        assert!(store.tracked_numbers(repo.id).unwrap().is_empty());

        // The user opens a PR found by search: it is added and tracked, unseen so far.
        let found = PolledPr {
            role: "other",
            ..polled(9, json!({"title": "found"}))
        };
        let id = store.track(repo.id, &found).unwrap();
        assert_eq!(store.item(id).unwrap().last_seen.as_deref(), Some(""));
        assert_eq!(store.tracked_numbers(repo.id).unwrap(), [9]);

        // Next poll: #9 is found by number (merged now), #5 is no longer requested.
        let merged = PolledPr {
            open: false,
            ..found
        };
        store.sync_prs(repo.id, &[merged]).unwrap();
        let items = store.items(repo.id).unwrap();
        let nine = items.iter().find(|i| i.number == 9).unwrap();
        let five = items.iter().find(|i| i.number == 5).unwrap();
        assert!(nine.tracked && !nine.open && nine.role == "other");
        assert!(
            !five.open,
            "an untracked PR no query finds drops out of the Inbox"
        );
    }

    #[test]
    fn sessions_track_start_and_transcript() {
        let (store, repo) = store_with_repo();
        store.sync_prs(repo.id, &[polled(7, json!({}))]).unwrap();
        let id = store.items(repo.id).unwrap()[0].id;
        store.set_session(id, "s-1").unwrap();
        assert!(!store.item(id).unwrap().session_started);
        store
            .mark_session_started(id, Some(Path::new("/t.jsonl")))
            .unwrap();
        store.mark_session_started(id, None).unwrap();
        let item = store.item(id).unwrap();
        assert_eq!(item.session_id.as_deref(), Some("s-1"));
        assert!(item.session_started);
        assert_eq!(item.transcript_path.as_deref(), Some(Path::new("/t.jsonl")));
    }
}
