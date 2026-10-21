// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

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
-- Why the item is in the Inbox: 'assignee' (assigned to the viewer), 'author' (the viewer's
-- PR), 'requested' (review requested from the viewer), 'reviewed' (the viewer reviewed it) or
-- 'other'. Recomputed on every poll.
ALTER TABLE work_items ADD COLUMN role TEXT NOT NULL DEFAULT 'author';
-- The user opened it, so it stays listed (and polled) even when no query finds it.
ALTER TABLE work_items ADD COLUMN tracked INTEGER NOT NULL DEFAULT 0;
"#,
    r#"
-- 'pr' or 'issue'. An issue item becomes a PR item (new number) when its branch is pushed.
ALTER TABLE work_items ADD COLUMN kind TEXT NOT NULL DEFAULT 'pr';
-- For a PR item that started as an issue: the issue's number.
ALTER TABLE work_items ADD COLUMN issue_number INTEGER;
-- Ariekei created the worktree (cleanup may delete it); adopted ones are only detached.
ALTER TABLE work_items ADD COLUMN worktree_created INTEGER NOT NULL DEFAULT 0;
-- Cleaned up or marked done: not shown in the Inbox until the user opens it again.
ALTER TABLE work_items ADD COLUMN hidden INTEGER NOT NULL DEFAULT 0;
-- Why automatic cleanup could not run (JSON array of messages), if it could not.
ALTER TABLE work_items ADD COLUMN cleanup_blockers TEXT;
"#,
    r#"
-- Per-file review marks: the content (git object id, or 'deleted') the user marked reviewed.
-- A file whose content changed since then is no longer reviewed.
CREATE TABLE reviewed_files (
    item_id INTEGER NOT NULL REFERENCES work_items(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    blob TEXT NOT NULL,
    PRIMARY KEY (item_id, path)
);
-- The latest agent review guide of an item, for the base/head it was made for.
CREATE TABLE review_guides (
    item_id INTEGER PRIMARY KEY REFERENCES work_items(id) ON DELETE CASCADE,
    base TEXT NOT NULL,
    head TEXT NOT NULL,
    guide TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
"#,
    r#"
-- The PR's commits GitHub attributes to the viewer (author or committer), for the PR head
-- they were fetched at: the user's own commits need no review from them.
CREATE TABLE own_commits (
    item_id INTEGER PRIMARY KEY REFERENCES work_items(id) ON DELETE CASCADE,
    head TEXT NOT NULL,
    commits TEXT NOT NULL
);
"#,
    r#"
-- Tasks: work started in Ariekei with no PR or issue yet (kind 'task'). A task's number is
-- negative (-1, -2, ... per repo) so it never collides with a GitHub number; its snapshot is
-- {title, notes, createdAt}. The PRs and issues its agent opens link to it here.
ALTER TABLE work_items ADD COLUMN task_id INTEGER REFERENCES work_items(id) ON DELETE SET NULL;
"#,
    r#"
-- The user starred it: it heads the Inbox (and stays tracked).
ALTER TABLE work_items ADD COLUMN starred INTEGER NOT NULL DEFAULT 0;
"#,
    r#"
-- The agent review guide is gone.
DROP TABLE review_guides;
"#,
    r#"
-- So are the reviewed marks: per file, per item (the reviewed commit), and the viewer's own
-- commits that told "changes to review" apart.
DROP TABLE reviewed_files;
DROP TABLE own_commits;
ALTER TABLE work_items DROP COLUMN reviewed_sha;
"#,
    r#"
-- Ariekei created the item's branch too, not only its worktree: cleanup may delete it. A
-- local branch it only reused (e.g. the user's main) is kept. Older rows keep the old rule.
ALTER TABLE work_items ADD COLUMN branch_created INTEGER NOT NULL DEFAULT 1;
"#,
    r#"
-- Transcripts of the item's earlier conversations (JSON array of paths), e.g. before a
-- `/clear` in its terminal; cleanup deletes them with the current one.
ALTER TABLE work_items ADD COLUMN past_transcripts TEXT;
"#,
    r#"
-- The item's conversation as GitHub last gave it (JSON): shown at once when the item opens,
-- while the current one loads.
ALTER TABLE work_items ADD COLUMN conversation TEXT;
"#,
    r#"
-- The user kept the finished item (it finished while they had it open): no automatic cleanup,
-- Done cleans it up.
ALTER TABLE work_items ADD COLUMN kept INTEGER NOT NULL DEFAULT 0;
"#,
    r#"
-- A task's PRs and issues are items like any other now: no longer linked to it (`task_id` is
-- unused, cleared at startup by `workitem::unlink_from_tasks`). Instead the one its agent
-- opened keeps how it came about (markdown: the task and its conversation up to then), for
-- the agent that later works on it.
ALTER TABLE work_items ADD COLUMN origin TEXT;
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
    pub last_seen: Option<String>,
    pub ui_state: Option<serde_json::Value>,
    pub role: String,
    pub tracked: bool,
    pub kind: String,
    pub issue_number: Option<i64>,
    pub worktree_created: bool,
    /// Ariekei created its branch too (cleanup may delete it).
    pub branch_created: bool,
    pub hidden: bool,
    pub cleanup_blockers: Option<Vec<String>>,
    /// For a PR or issue a task's agent opened: how it came about (`context::render_origin`).
    pub origin: Option<String>,
    pub starred: bool,
    /// Transcripts of the item's earlier conversations (see `switch_session`).
    pub past_transcripts: Vec<PathBuf>,
    /// Finished, and the user chose to keep it until Done: no automatic cleanup.
    pub kept: bool,
}

impl WorkItem {
    pub fn is_task(&self) -> bool {
        self.kind == "task"
    }

    pub fn title(&self) -> &str {
        self.snapshot["title"].as_str().unwrap_or_default()
    }

    /// How the item is named to the user: `T1 title` for a task, `#12 title` otherwise.
    pub fn label(&self) -> String {
        format!("{} {}", self.short_name(), self.title())
    }

    /// `T1` for a task, `#12` otherwise.
    pub fn short_name(&self) -> String {
        if self.is_task() {
            format!("T{}", -self.number)
        } else {
            format!("#{}", self.number)
        }
    }

    /// The item in file and ref names: `task1`, `issue5`, `pr12`.
    pub fn slug(&self) -> String {
        if self.is_task() {
            format!("task{}", -self.number)
        } else {
            format!("{}{}", self.kind, self.number)
        }
    }
}

/// One PR as found by a poll.
#[derive(Debug, Clone)]
pub struct PolledPr {
    pub number: i64,
    /// "pr" or "issue" (tasks are never polled).
    pub kind: &'static str,
    pub role: &'static str,
    pub open: bool,
    pub snapshot: serde_json::Value,
}

/// What the previous poll knew about an item: whether it found it (open), why it is in the
/// Inbox, and its state on GitHub.
#[derive(Debug, Clone)]
struct Seen {
    open: bool,
    role: String,
    state: String,
}

/// Whether what a poll found brings a hidden item (cleaned up, or marked done) back into the
/// Inbox: it was reopened, or it has a reason for the user to look at it again that it did
/// not have when the previous poll found it (their review is requested, it is assigned to
/// them). A reason it already had does not, or Done on an open PR would undo itself on the
/// next poll.
fn brings_back(seen: &Seen, found: &PolledPr) -> bool {
    let reopened = seen.state != "OPEN";
    let new_reason =
        matches!(found.role, "requested" | "assignee") && !(seen.open && seen.role == found.role);
    found.open && (reopened || new_reason)
}

pub struct Store {
    conn: Mutex<Connection>,
}

/// An item's `last_seen` when the user (or a poll) saw it with no activity yet: it sorts before
/// every time, so all activity after counts as new. `""` is never seen (e.g. added from Browse).
pub const NOTHING_SEEN: &str = "0";

fn err(e: rusqlite::Error) -> String {
    e.to_string()
}

const ITEM_COLUMNS: &str = "id, repo_id, number, snapshot, open, worktree_path, branch, session_id, \
     session_started, transcript_path, last_seen, ui_state, role, tracked, kind, \
     issue_number, worktree_created, hidden, cleanup_blockers, origin, starred, branch_created, \
     past_transcripts, kept";

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
        last_seen: r.get(10)?,
        ui_state: json(r.get(11)?),
        role: r.get(12)?,
        tracked: r.get(13)?,
        kind: r.get(14)?,
        issue_number: r.get(15)?,
        worktree_created: r.get(16)?,
        hidden: r.get(17)?,
        cleanup_blockers: r
            .get::<_, Option<String>>(18)?
            .and_then(|s| serde_json::from_str(&s).ok()),
        origin: r.get(19)?,
        starred: r.get(20)?,
        branch_created: r.get(21)?,
        past_transcripts: r
            .get::<_, Option<String>>(22)?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default(),
        kept: r.get(23)?,
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

    /// The repo's owner and name on GitHub (they change when it is renamed or transferred).
    pub fn set_slug(&self, id: i64, owner: &str, name: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE repos SET owner = ?2, name = ?3 WHERE id = ?1",
            params![id, owner, name],
        )
        .map(|_| ())
        .map_err(err)
    }

    /// The GitHub login the repo is polled for (who gh is logged in as).
    pub fn set_viewer(&self, id: i64, viewer: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE repos SET viewer = ?2 WHERE id = ?1",
            params![id, viewer],
        )
        .map(|_| ())
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

    /// All connected repos, by name.
    pub fn repos(&self) -> Result<Vec<Repo>, String> {
        let ids: Vec<i64> = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn
                .prepare("SELECT id FROM repos ORDER BY owner, name, path")
                .map_err(err)?;
            stmt.query_map([], |r| r.get(0))
                .map_err(err)?
                .collect::<Result<_, _>>()
                .map_err(err)?
        };
        ids.into_iter()
            .filter_map(|id| self.repo(id).transpose())
            .collect()
    }

    // --- work items ---

    /// Records what a poll found. Items it did not find are kept (with `open = false`)
    /// until cleanup, so no local state is lost. A hidden item comes back if what was found
    /// brings it back (`brings_back`).
    #[cfg(test)]
    pub fn sync_prs(&self, repo_id: i64, prs: &[PolledPr]) -> Result<(), String> {
        self.sync_found(repo_id, prs, &[])
    }

    /// `sync_prs`, where the searches for items of the kinds in `cut_short` ("pr", "issue")
    /// did not read all results: items of those kinds that were not found may be among the
    /// rest, so they keep what they had.
    pub fn sync_found(
        &self,
        repo_id: i64,
        prs: &[PolledPr],
        cut_short: &[&str],
    ) -> Result<(), String> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction().map_err(err)?;
        let hidden: Vec<(i64, Seen)> = {
            let mut stmt = tx
                .prepare(
                    "SELECT number, open, role, json_extract(snapshot, '$.state') FROM work_items
                     WHERE repo_id = ?1 AND hidden = 1",
                )
                .map_err(err)?;
            stmt.query_map([repo_id], |r| {
                Ok((
                    r.get(0)?,
                    Seen {
                        open: r.get(1)?,
                        role: r.get(2)?,
                        state: r
                            .get::<_, Option<String>>(3)?
                            .unwrap_or_else(|| "OPEN".into()),
                    },
                ))
            })
            .map_err(err)?
            .collect::<Result<_, _>>()
            .map_err(err)?
        };
        for kind in ["pr", "issue"].iter().filter(|k| !cut_short.contains(k)) {
            tx.execute(
                "UPDATE work_items SET open = 0 WHERE repo_id = ?1 AND kind = ?2",
                params![repo_id, kind],
            )
            .map_err(err)?;
        }
        for pr in prs {
            tx.execute(
                "INSERT INTO work_items (repo_id, number, snapshot, open, role, kind) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (repo_id, number) DO UPDATE SET snapshot = ?3, open = ?4, role = ?5, kind = ?6",
                params![repo_id, pr.number, pr.snapshot.to_string(), pr.open, pr.role, pr.kind],
            )
            .map_err(err)?;
            if hidden
                .iter()
                .any(|(number, seen)| *number == pr.number && brings_back(seen, pr))
            {
                tx.execute(
                    "UPDATE work_items SET hidden = 0 WHERE repo_id = ?1 AND number = ?2",
                    params![repo_id, pr.number],
                )
                .map_err(err)?;
            }
        }
        tx.commit().map_err(err)
    }

    /// Adds a PR or issue the user picked (e.g. from search) or a task's agent opened, or marks
    /// an existing one as tracked.
    pub fn track(&self, repo_id: i64, pr: &PolledPr) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO work_items (repo_id, number, snapshot, open, role, tracked, last_seen, kind)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, '', ?6)
             ON CONFLICT (repo_id, number) DO UPDATE SET snapshot = ?3, open = ?4, tracked = 1, hidden = 0",
            params![
                repo_id,
                pr.number,
                pr.snapshot.to_string(),
                pr.open,
                pr.role,
                pr.kind
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

    /// Stars or unstars an item. A starred item is tracked, so it stays listed when no query
    /// finds it any more.
    pub fn set_starred(&self, id: i64, starred: bool) -> Result<(), String> {
        if starred {
            self.update(id, "starred = 1, tracked = ?2", true)
        } else {
            self.update(id, "starred = ?2", false)
        }
    }

    /// Takes an item the user added out of the Inbox: it is no longer polled by number, and
    /// comes back only when a query finds it (e.g. a new review request).
    pub fn untrack(&self, id: i64) -> Result<(), String> {
        self.update(id, "tracked = ?2, starred = 0, open = 0", false)
    }

    /// Numbers of the tracked items of `kind` ("pr" or "issue") that are still listed:
    /// cleaned-up ones are finished, and polling them would cost more with every item.
    pub fn tracked_numbers(&self, repo_id: i64, kind: &str) -> Result<Vec<i64>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT number FROM work_items WHERE repo_id = ?1 AND tracked = 1 AND kind = ?2 AND hidden = 0",
            )
            .map_err(err)?;
        let rows = stmt
            .query_map(params![repo_id, kind], |r| r.get(0))
            .map_err(err)?;
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

    /// `created`: Ariekei made the worktree (cleanup may delete it) rather than adopting it.
    pub fn set_worktree(
        &self,
        id: i64,
        path: Option<&Path>,
        branch: Option<&str>,
        created: bool,
        branch_created: bool,
    ) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE work_items SET worktree_path = ?2, branch = ?3, worktree_created = ?4, branch_created = ?5 WHERE id = ?1",
            params![id, path.map(path_str), branch, created, branch_created],
        )
        .map(|_| ())
        .map_err(err)
    }

    /// The issue item `id` now is PR `number` (opened for its branch).
    pub fn convert_to_pr(
        &self,
        id: i64,
        number: i64,
        snapshot: &serde_json::Value,
    ) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE work_items SET issue_number = number, number = ?2, kind = 'pr', role = 'author',
             snapshot = ?3, tracked = 1, open = 1 WHERE id = ?1",
            params![id, number, snapshot.to_string()],
        )
        .map(|_| ())
        .map_err(err)
    }

    /// A new task (see the migration for how tasks are stored); returns its id. `head` is
    /// the existing branch it works on, if any (`worktree::BranchRef`).
    pub fn create_task(
        &self,
        repo_id: i64,
        title: &str,
        notes: &str,
        head: Option<&serde_json::Value>,
    ) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();
        let number: i64 = conn
            .query_row(
                "SELECT COALESCE(MIN(number), 0) - 1 FROM work_items WHERE repo_id = ?1 AND number < 0",
                [repo_id],
                |r| r.get(0),
            )
            .map_err(err)?;
        conn.execute(
            "INSERT INTO work_items (repo_id, number, snapshot, open, role, kind, last_seen)
             VALUES (?1, ?2, json_patch(json_object('title', ?3, 'notes', ?4,
                                         'createdAt', strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                                        json_object('head', json(?5))),
                     1, 'author', 'task', '')",
            params![repo_id, number, title, notes, head.map(|h| h.to_string())],
        )
        .map_err(err)?;
        Ok(conn.last_insert_rowid())
    }

    /// Keeps how PR or issue `id` came about, unless it knows already.
    pub fn set_origin(&self, id: i64, origin: &str) -> Result<(), String> {
        self.update(id, "origin = COALESCE(origin, ?2)", origin)
    }

    /// Items still linked to a task (the old way), with their task's id.
    pub fn linked_to_tasks(&self) -> Result<Vec<(i64, i64)>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, task_id FROM work_items WHERE task_id IS NOT NULL")
            .map_err(err)?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(err)?
            .collect::<Result<_, _>>()
            .map_err(err)
    }

    pub fn unlink_from_task(&self, id: i64) -> Result<(), String> {
        self.update(id, "task_id = ?2", None::<i64>)
    }

    /// Links item `id` to a task the old way (`linked_to_tasks`).
    #[cfg(test)]
    pub fn link_to_task(&self, id: i64, task_id: i64) -> Result<(), String> {
        self.update(id, "task_id = ?2", task_id)
    }

    /// Forgets a connected repo with all its work items (and what hangs on them).
    pub fn delete_repo(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM repos WHERE id = ?1", [id])
            .map(|_| ())
            .map_err(err)
    }

    pub fn delete_item(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM work_items WHERE id = ?1", [id])
            .map(|_| ())
            .map_err(err)
    }

    pub fn set_cleanup_blockers(&self, id: i64, blockers: Option<&[String]>) -> Result<(), String> {
        let value = blockers.map(|b| serde_json::to_string(b).unwrap_or_default());
        self.update(id, "cleanup_blockers = ?2", value)
    }

    /// The user keeps finished item `id` until Done: no automatic cleanup (`false`: it is
    /// open again).
    pub fn set_kept(&self, id: i64, kept: bool) -> Result<(), String> {
        self.update(id, "kept = ?2", kept)
    }

    /// Item `id` is gone from GitHub (deleted, or moved to another repository): it counts as
    /// closed.
    pub fn mark_gone(&self, id: i64) -> Result<(), String> {
        self.update(id, "snapshot = json_set(snapshot, '$.state', ?2)", "CLOSED")
    }

    /// After cleanup: forget the worktree and agent session, and hide the item.
    pub fn mark_cleaned(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE work_items SET worktree_path = NULL, branch = NULL, worktree_created = 0, session_id = NULL,
             session_started = 0, transcript_path = NULL, past_transcripts = NULL, cleanup_blockers = NULL,
             kept = 0, hidden = 1 WHERE id = ?1",
            [id],
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

    /// The item's agent went on in a new conversation, `session_id` (after `/clear`): it is
    /// the one to resume from now on, and the transcript of the one before is kept with the
    /// item's past ones, for cleanup.
    pub fn switch_session(&self, id: i64, session_id: &str) -> Result<(), String> {
        let item = self.item(id)?;
        let mut past = item.past_transcripts;
        if let Some(transcript) = item.transcript_path
            && !past.contains(&transcript)
        {
            past.push(transcript);
        }
        let past = serde_json::to_string(&past).unwrap_or_default();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE work_items SET session_id = ?2, session_started = 0, transcript_path = NULL,
             past_transcripts = ?3 WHERE id = ?1",
            params![id, session_id, past],
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

    /// `at`: GitHub's time of the latest activity the user has seen, or `NOTHING_SEEN`.
    pub fn set_last_seen(&self, id: i64, at: &str) -> Result<(), String> {
        self.update(id, "last_seen = ?2", at)
    }

    pub fn set_ui_state(&self, id: i64, state: &serde_json::Value) -> Result<(), String> {
        self.update(id, "ui_state = ?2", state.to_string())
    }

    /// The item's conversation as it was last loaded (see `set_conversation`).
    pub fn conversation(&self, id: i64) -> Result<Option<serde_json::Value>, String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT conversation FROM work_items WHERE id = ?1",
            [id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(err)
        .map(|s| s.flatten().and_then(|s| serde_json::from_str(&s).ok()))
    }

    pub fn set_conversation(
        &self,
        id: i64,
        conversation: &serde_json::Value,
    ) -> Result<(), String> {
        self.update(id, "conversation = ?2", conversation.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn polled(number: i64, snapshot: serde_json::Value) -> PolledPr {
        PolledPr {
            number,
            kind: "pr",
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
    fn repos_are_listed_and_reconnect_keeps_id() {
        let (store, repo) = store_with_repo();
        let other = store
            .add_repo(Path::new("/src/aaa"), "aaa", "x", "origin", "me")
            .unwrap();
        assert_eq!(store.repos().unwrap(), [other, repo.clone()]);
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
            .set_worktree(
                one.id,
                Some(Path::new("/src/besu_pr1")),
                Some("feat"),
                true,
                true,
            )
            .unwrap();
        store.set_session(one.id, "s-1").unwrap();

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
        assert_eq!(one.session_id.as_deref(), Some("s-1"));
    }

    #[test]
    fn a_search_cut_short_closes_nothing_of_its_kind() {
        let (store, repo) = store_with_repo();
        let issue = PolledPr {
            kind: "issue",
            ..polled(2, json!({"title": "b"}))
        };
        store
            .sync_prs(repo.id, &[polled(1, json!({"title": "a"})), issue])
            .unwrap();
        // Neither is found: the PR searches did not read all their results, the issue's did.
        store.sync_found(repo.id, &[], &["pr"]).unwrap();
        let open = |number| {
            store
                .items(repo.id)
                .unwrap()
                .iter()
                .any(|i| i.number == number && i.open)
        };
        assert!(open(1) && !open(2));
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
        assert!(store.tracked_numbers(repo.id, "pr").unwrap().is_empty());

        // The user opens a PR found by search: it is added and tracked, unseen so far.
        let found = PolledPr {
            role: "other",
            ..polled(9, json!({"title": "found"}))
        };
        let id = store.track(repo.id, &found).unwrap();
        assert_eq!(store.item(id).unwrap().last_seen.as_deref(), Some(""));
        assert_eq!(store.tracked_numbers(repo.id, "pr").unwrap(), [9]);

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

        // `/clear`: a new conversation; the old transcript stays the item's, for cleanup.
        store.switch_session(id, "s-2").unwrap();
        let item = store.item(id).unwrap();
        assert_eq!(
            (
                item.session_id.as_deref(),
                item.session_started,
                item.transcript_path
            ),
            (Some("s-2"), false, None)
        );
        assert_eq!(item.past_transcripts, [PathBuf::from("/t.jsonl")]);
        store
            .mark_session_started(id, Some(Path::new("/t2.jsonl")))
            .unwrap();
        store.switch_session(id, "s-3").unwrap();
        assert_eq!(
            store.item(id).unwrap().past_transcripts,
            [PathBuf::from("/t.jsonl"), PathBuf::from("/t2.jsonl")]
        );
        store.mark_cleaned(id).unwrap();
        assert!(store.item(id).unwrap().past_transcripts.is_empty());
    }

    #[test]
    fn tasks_get_negative_numbers_stay_open_and_lose_their_links_when_deleted() {
        let (store, repo) = store_with_repo();
        let t1 = store
            .create_task(repo.id, "Flaky sync", "look at peers", None)
            .unwrap();
        let t2 = store.create_task(repo.id, "Slow start", "", None).unwrap();
        let task = store.item(t1).unwrap();
        assert_eq!((task.kind.as_str(), task.number), ("task", -1));
        assert_eq!(task.snapshot["title"], "Flaky sync");
        assert_eq!(task.snapshot["notes"], "look at peers");
        assert!(task.snapshot["createdAt"].as_str().unwrap().ends_with('Z'));
        assert_eq!(store.item(t2).unwrap().number, -2);

        // A poll knows nothing about tasks and must not close them.
        store
            .sync_prs(repo.id, &[polled(7, json!({"title": "pr"}))])
            .unwrap();
        assert!(store.item(t1).unwrap().open);

        let pr = store
            .track(repo.id, &polled(7, json!({"title": "pr"})))
            .unwrap();
        store.set_origin(pr, "from T1").unwrap();
        store.set_origin(pr, "from T2").unwrap();
        assert_eq!(
            store.item(pr).unwrap().origin.as_deref(),
            Some("from T1"),
            "the first origin stays"
        );
        // The next task does not reuse a live number.
        assert_eq!(
            store
                .item(store.create_task(repo.id, "x", "", None).unwrap())
                .unwrap()
                .number,
            -3
        );
    }

    #[test]
    fn an_older_database_is_upgraded_with_its_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.db");
        // A database as the release before tasks left it: migrations 0-4 and a PR row.
        {
            let conn = Connection::open(&path).unwrap();
            for sql in &MIGRATIONS[..5] {
                conn.execute_batch(sql).unwrap();
            }
            conn.pragma_update(None, "user_version", 5).unwrap();
            conn.execute(
                "INSERT INTO repos (path, owner, name, base_remote, viewer) VALUES ('/r', 'o', 'n', 'origin', 'me')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO work_items (repo_id, number, snapshot, kind, tracked, worktree_path)
                 VALUES (1, 7, '{\"title\": \"old\"}', 'pr', 1, '/w')",
                [],
            )
            .unwrap();
        }
        let store = Store::open(&path).unwrap();
        let items = store.items(1).unwrap();
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(
            (
                item.number,
                item.title(),
                item.origin.as_deref(),
                item.worktree_path.as_deref()
            ),
            (7, "old", None, Some(Path::new("/w")))
        );
        assert!(!item.starred);
        assert_eq!(store.linked_to_tasks().unwrap(), []);
    }

    #[test]
    fn cleaned_up_items_are_no_longer_polled_until_tracked_again() {
        let (store, repo) = store_with_repo();
        let id = store
            .track(repo.id, &polled(7, json!({"title": "a"})))
            .unwrap();
        store
            .track(repo.id, &polled(8, json!({"title": "b"})))
            .unwrap();
        assert_eq!(store.tracked_numbers(repo.id, "pr").unwrap(), [7, 8]);
        store.mark_cleaned(id).unwrap();
        let cleaned = store.item(id).unwrap();
        assert!(cleaned.hidden && cleaned.tracked);
        assert_eq!(store.tracked_numbers(repo.id, "pr").unwrap(), [8]);
        // Opening it again (e.g. from search) brings it back, polled.
        store
            .track(repo.id, &polled(7, json!({"title": "a"})))
            .unwrap();
        assert!(!store.item(id).unwrap().hidden);
        assert_eq!(store.tracked_numbers(repo.id, "pr").unwrap(), [7, 8]);
    }

    #[test]
    fn a_hidden_item_comes_back_when_reopened_or_asked_for_again() {
        let (store, repo) = store_with_repo();
        let found = |role: &'static str, state: &str| PolledPr {
            role,
            open: state == "OPEN",
            ..polled(5, json!({"title": "t", "state": state}))
        };
        let hidden = |id| store.item(id).unwrap().hidden;
        // Done on a PR the user reviewed: later polls find it as before, it stays hidden.
        store
            .sync_prs(repo.id, &[found("reviewed", "OPEN")])
            .unwrap();
        let id = store.items(repo.id).unwrap()[0].id;
        store.mark_cleaned(id).unwrap();
        store
            .sync_prs(repo.id, &[found("reviewed", "OPEN")])
            .unwrap();
        assert!(hidden(id));
        // The author asks for their review again.
        store
            .sync_prs(repo.id, &[found("requested", "OPEN")])
            .unwrap();
        assert!(!hidden(id));

        // Done while the request stands: hidden while it does; asked again later: back.
        store.mark_cleaned(id).unwrap();
        store
            .sync_prs(repo.id, &[found("requested", "OPEN")])
            .unwrap();
        assert!(hidden(id));
        store.sync_prs(repo.id, &[]).unwrap();
        assert!(hidden(id));
        store
            .sync_prs(repo.id, &[found("requested", "OPEN")])
            .unwrap();
        assert!(!hidden(id));

        // The user's own PR, closed and cleaned up, then reopened.
        store
            .sync_prs(repo.id, &[found("author", "CLOSED")])
            .unwrap();
        store.mark_cleaned(id).unwrap();
        store
            .sync_prs(repo.id, &[found("author", "CLOSED")])
            .unwrap();
        assert!(hidden(id));
        store.sync_prs(repo.id, &[found("author", "OPEN")]).unwrap();
        assert!(!hidden(id));
    }

    #[test]
    fn an_issue_becomes_its_pr_keeping_its_work() {
        let (store, repo) = store_with_repo();
        let issue = PolledPr {
            kind: "issue",
            role: "assignee",
            ..polled(5, json!({"title": "Crash"}))
        };
        store.sync_prs(repo.id, &[issue]).unwrap();
        let id = store.items(repo.id).unwrap()[0].id;
        store
            .set_worktree(id, Some(Path::new("/w")), Some("issue-5-crash"), true, true)
            .unwrap();
        store.set_session(id, "s-1").unwrap();
        store
            .convert_to_pr(id, 100, &json!({"title": "Crash fix"}))
            .unwrap();
        let pr = store.item(id).unwrap();
        assert_eq!(
            (
                pr.kind.as_str(),
                pr.number,
                pr.issue_number,
                pr.role.as_str()
            ),
            ("pr", 100, Some(5), "author")
        );
        assert!(pr.tracked && pr.open);
        assert_eq!(pr.title(), "Crash fix");
        assert_eq!(
            (
                pr.worktree_path.as_deref(),
                pr.branch.as_deref(),
                pr.session_id.as_deref()
            ),
            (Some(Path::new("/w")), Some("issue-5-crash"), Some("s-1"))
        );
        assert_eq!(store.tracked_numbers(repo.id, "pr").unwrap(), [100]);
    }
}
