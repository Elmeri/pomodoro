use anyhow::{Context, Result};
use chrono::{DateTime, Local, TimeZone};
use rusqlite::{params, Connection};
use std::fs;
use std::path::PathBuf;

pub struct Database {
    conn: Connection,
}

pub struct ProjectStat {
    pub project: String,
    pub total_minutes: i64,
    pub session_count: i64,
}

impl Database {
    pub fn init() -> Result<Self> {
        let db_path = Self::get_db_path()?;
        let conn = Connection::open(&db_path)
            .with_context(|| format!("Failed to open database at {:?}", db_path))?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS sessions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project TEXT NOT NULL,
                duration_mins INTEGER NOT NULL,
                completed_at TEXT NOT NULL
            )",
            [],
        )?;

        Ok(Self { conn })
    }

    fn get_db_path() -> Result<PathBuf> {
        let mut path = dirs::data_local_dir().context("Could not find local data directory")?;
        path.push("pomo");
        fs::create_dir_all(&path)?;
        path.push("pomo.db");
        Ok(path)
    }

    pub fn log_session(&self, project: &str, duration_mins: u64) -> Result<()> {
        let now: DateTime<Local> = Local::now();
        self.conn.execute(
            "INSERT INTO sessions (project, duration_mins, completed_at) VALUES (?1, ?2, ?3)",
            params![project, duration_mins as i64, now.to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn get_stats(&self, since: Option<DateTime<Local>>) -> Result<Vec<ProjectStat>> {
        let mut stats = Vec::new();

        if let Some(cutoff) = since {
            let mut stmt = self.conn.prepare(
                "SELECT project, SUM(duration_mins), COUNT(id)
                 FROM sessions
                 WHERE completed_at >= ?1
                 GROUP BY project
                 ORDER BY SUM(duration_mins) DESC",
            )?;

            let rows = stmt.query_map(params![cutoff.to_rfc3339()], |row| {
                Ok(ProjectStat {
                    project: row.get(0)?,
                    total_minutes: row.get(1)?,
                    session_count: row.get(2)?,
                })
            })?;

            for stat in rows {
                stats.push(stat?);
            }
        } else {
            let mut stmt = self.conn.prepare(
                "SELECT project, SUM(duration_mins), COUNT(id)
                 FROM sessions
                 GROUP BY project
                 ORDER BY SUM(duration_mins) DESC",
            )?;

            let rows = stmt.query_map([], |row| {
                Ok(ProjectStat {
                    project: row.get(0)?,
                    total_minutes: row.get(1)?,
                    session_count: row.get(2)?,
                })
            })?;

            for stat in rows {
                stats.push(stat?);
            }
        }

        Ok(stats)
    }

    /// Convenience helper for today's stats (since midnight local time)
    pub fn get_today_stats(&self) -> Result<Vec<ProjectStat>> {
        let now = Local::now();
        let midnight = now.date_naive().and_hms_opt(0, 0, 0).unwrap();
        let since = Local.from_local_datetime(&midnight).single();
        self.get_stats(since)
    }

    /// Fetch the most recently used project names for quick-select chips & tray menu
    pub fn get_recent_projects(&self, limit: usize) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT project
             FROM sessions
             GROUP BY project
             ORDER BY MAX(completed_at) DESC
             LIMIT ?1",
        )?;

        let rows = stmt.query_map(params![limit as i64], |row| row.get::<_, String>(0))?;

        let mut projects = Vec::new();
        for proj in rows {
            projects.push(proj?);
        }
        Ok(projects)
    }
}