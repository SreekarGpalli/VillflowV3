use std::path::{Path, PathBuf};
use std::fs;
use rusqlite::OptionalExtension;
use vf_core::{DictEntry, HistoryEntry, InsightsSummary, Settings, Store};

// --- SETTINGS LOAD / SAVE ---

pub fn get_default_settings_path() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("VillFlow").join("settings.json"))
}

pub fn get_default_db_path() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("VillFlow").join("villflow.db"))
}

pub fn load_settings(path: &Path) -> anyhow::Result<Settings> {
    if !path.exists() {
        let settings = Settings::default();
        save_settings(&settings, path)?;
        return Ok(settings);
    }
    
    let content = fs::read_to_string(path)?;
    // If JSON is empty, fallback to default
    if content.trim().is_empty() {
        let settings = Settings::default();
        save_settings(&settings, path)?;
        return Ok(settings);
    }

    // Missing fields on load -> filled from defaults via serde default attributes, then re-saved.
    match serde_json::from_str::<Settings>(&content) {
        Ok(settings) => {
            save_settings(&settings, path)?;
            Ok(settings)
        }
        Err(e) => {
            // Corrupt / invalid JSON: back up and recover with defaults so the app can launch.
            let backup = path.with_extension("json.corrupt");
            let _ = fs::copy(path, &backup);
            log::warn!(
                "settings.json unreadable ({e}); backed up to {} and restoring defaults",
                backup.display()
            );
            let settings = Settings::default();
            save_settings(&settings, path)?;
            Ok(settings)
        }
    }
}

pub fn save_settings(settings: &Settings, path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    
    let tmp_path = path.with_extension("tmp");
    let serialized = serde_json::to_string_pretty(settings)?;
    if let Err(e) = fs::write(&tmp_path, serialized) {
        let _ = fs::remove_file(&tmp_path);
        return Err(e.into());
    }
    if let Err(e) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(e.into());
    }
    Ok(())
}

// --- SQLITE STORE ---

pub struct SqliteStore {
    conn: std::sync::Mutex<rusqlite::Connection>,
}

impl SqliteStore {
    pub fn new(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let conn = rusqlite::Connection::open(path)?;
        let store = Self {
            conn: std::sync::Mutex::new(conn),
        };
        store.init_db()?;
        Ok(store)
    }

    pub fn new_in_memory() -> anyhow::Result<Self> {
        let conn = rusqlite::Connection::open_in_memory()?;
        let store = Self {
            conn: std::sync::Mutex::new(conn),
        };
        store.init_db()?;
        Ok(store)
    }

    fn init_db(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("DB Mutex poisoned"))?;
        
        conn.execute(
            "CREATE TABLE IF NOT EXISTS dictionary (
              id INTEGER PRIMARY KEY,
              word TEXT NOT NULL UNIQUE,
              starred INTEGER NOT NULL DEFAULT 0,
              source TEXT NOT NULL DEFAULT 'manual',
              use_count INTEGER NOT NULL DEFAULT 0,
              created_at TEXT NOT NULL
            );",
            [],
        )?;
        // Case-insensitive uniqueness for dictionary words (best-effort if legacy dups exist).
        let _ = conn.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS dictionary_word_nocase
             ON dictionary(word COLLATE NOCASE);",
            [],
        );
        
        conn.execute(
            "CREATE TABLE IF NOT EXISTS history (
              id INTEGER PRIMARY KEY,
              ts TEXT NOT NULL,
              app_name TEXT NOT NULL,
              window_title TEXT NOT NULL DEFAULT '',
              mode TEXT NOT NULL DEFAULT 'dictation',
              raw_transcript TEXT NOT NULL,
              final_text TEXT NOT NULL,
              duration_ms INTEGER NOT NULL DEFAULT 0,
              word_count INTEGER NOT NULL DEFAULT 0
            );",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS history_ts_idx ON history(ts);",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS history_mode_idx ON history(mode);",
            [],
        )?;
        
        conn.execute(
            "CREATE TABLE IF NOT EXISTS scratchpad (
              id INTEGER PRIMARY KEY CHECK (id = 1),
              content TEXT NOT NULL DEFAULT '',
              updated_at TEXT NOT NULL
            );",
            [],
        )?;
        
        // Initialize scratchpad row if not exists
        conn.execute(
            "INSERT OR IGNORE INTO scratchpad (id, content, updated_at) 
             VALUES (1, '', strftime('%Y-%m-%dT%H:%M:%S', 'now', 'localtime'));",
            [],
        )?;

        let _ = conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;");
        
        Ok(())
    }
}

impl Store for SqliteStore {
    fn dictionary_list(&self) -> anyhow::Result<Vec<DictEntry>> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("DB Mutex poisoned"))?;
        let mut stmt = conn.prepare(
            "SELECT id, word, starred, source, use_count FROM dictionary ORDER BY created_at DESC"
        )?;
        let rows = stmt.query_map([], |row| {
            let starred_int: i32 = row.get(2)?;
            Ok(DictEntry {
                id: row.get(0)?,
                word: row.get(1)?,
                starred: starred_int != 0,
                source: row.get(3)?,
                use_count: row.get(4)?,
            })
        })?;
        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    fn dictionary_add(&self, word: &str, source: &str) -> anyhow::Result<DictEntry> {
        let word = word.trim();
        if word.is_empty() {
            anyhow::bail!("dictionary word cannot be empty");
        }
        let source = match source.trim() {
            "auto" => "auto",
            _ => "manual",
        };
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("DB Mutex poisoned"))?;
        conn.execute(
            "INSERT INTO dictionary (word, source, created_at) VALUES (?, ?, strftime('%Y-%m-%dT%H:%M:%S', 'now', 'localtime'))",
            rusqlite::params![word, source],
        ).map_err(|e| {
            if let rusqlite::Error::SqliteFailure(code, _) = &e {
                if code.code == rusqlite::ErrorCode::ConstraintViolation {
                    return anyhow::anyhow!("dictionary word already exists: {word}");
                }
            }
            anyhow::anyhow!(e)
        })?;
        let id = conn.last_insert_rowid();
        Ok(DictEntry {
            id,
            word: word.to_string(),
            starred: false,
            source: source.to_string(),
            use_count: 0,
        })
    }

    fn dictionary_delete(&self, id: i64) -> anyhow::Result<()> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("DB Mutex poisoned"))?;
        let n = conn.execute("DELETE FROM dictionary WHERE id = ?", rusqlite::params![id])?;
        if n == 0 {
            anyhow::bail!("dictionary entry not found: {id}");
        }
        Ok(())
    }

    fn dictionary_update(&self, id: i64, word: &str) -> anyhow::Result<()> {
        let word = word.trim();
        if word.is_empty() {
            anyhow::bail!("dictionary word cannot be empty");
        }
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("DB Mutex poisoned"))?;
        let n = conn.execute(
            "UPDATE dictionary SET word = ? WHERE id = ?",
            rusqlite::params![word, id],
        )?;
        if n == 0 {
            anyhow::bail!("dictionary entry not found: {id}");
        }
        Ok(())
    }

    fn dictionary_toggle_star(&self, id: i64) -> anyhow::Result<()> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("DB Mutex poisoned"))?;
        let n = conn.execute(
            "UPDATE dictionary SET starred = CASE WHEN starred = 0 THEN 1 ELSE 0 END WHERE id = ?",
            rusqlite::params![id],
        )?;
        if n == 0 {
            anyhow::bail!("dictionary entry not found: {id}");
        }
        Ok(())
    }

    fn dictionary_bump_use_count(&self, words: &[String]) -> anyhow::Result<()> {
        // Dedupe case-insensitively so one appearance per utterance → one bump.
        let mut seen = std::collections::HashSet::new();
        let unique: Vec<&str> = words
            .iter()
            .map(|w| w.trim())
            .filter(|w| !w.is_empty())
            .filter(|w| seen.insert(w.to_ascii_lowercase()))
            .collect();
        if unique.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().map_err(|_| anyhow::anyhow!("DB Mutex poisoned"))?;
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare("UPDATE dictionary SET use_count = use_count + 1 WHERE word = ? COLLATE NOCASE")?;
            for word in unique {
                stmt.execute(rusqlite::params![word])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn history_append(&self, entry: &HistoryEntry) -> anyhow::Result<()> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("DB Mutex poisoned"))?;
        if entry.ts.is_empty() {
            conn.execute(
                "INSERT INTO history (ts, app_name, window_title, mode, raw_transcript, final_text, duration_ms, word_count) 
                 VALUES (strftime('%Y-%m-%dT%H:%M:%S', 'now', 'localtime'), ?, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![
                    entry.app_name,
                    entry.window_title,
                    entry.mode,
                    entry.raw_transcript,
                    entry.final_text,
                    entry.duration_ms,
                    entry.word_count,
                ],
            )?;
        } else {
            conn.execute(
                "INSERT INTO history (ts, app_name, window_title, mode, raw_transcript, final_text, duration_ms, word_count) 
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![
                    entry.ts,
                    entry.app_name,
                    entry.window_title,
                    entry.mode,
                    entry.raw_transcript,
                    entry.final_text,
                    entry.duration_ms,
                    entry.word_count,
                ],
            )?;
        }
        Ok(())
    }

    fn history_list(&self, limit: u32, offset: u32) -> anyhow::Result<Vec<HistoryEntry>> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("DB Mutex poisoned"))?;
        let mut stmt = conn.prepare(
            "SELECT id, ts, app_name, window_title, mode, raw_transcript, final_text, duration_ms, word_count 
             FROM history ORDER BY ts DESC, id DESC LIMIT ? OFFSET ?"
        )?;
        let rows = stmt.query_map(rusqlite::params![limit, offset], |row| {
            Ok(HistoryEntry {
                id: Some(row.get(0)?),
                ts: row.get(1)?,
                app_name: row.get(2)?,
                window_title: row.get(3)?,
                mode: row.get(4)?,
                raw_transcript: row.get(5)?,
                final_text: row.get(6)?,
                duration_ms: row.get(7)?,
                word_count: row.get(8)?,
            })
        })?;
        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    fn scratchpad_get(&self) -> anyhow::Result<String> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("DB Mutex poisoned"))?;
        let content: Option<String> = conn.query_row(
            "SELECT content FROM scratchpad WHERE id = 1",
            [],
            |row| row.get(0),
        ).optional()?;
        Ok(content.unwrap_or_default())
    }

    fn scratchpad_set(&self, content: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("DB Mutex poisoned"))?;
        conn.execute(
            "INSERT OR REPLACE INTO scratchpad (id, content, updated_at) VALUES (1, ?, strftime('%Y-%m-%dT%H:%M:%S', 'now', 'localtime'))",
            rusqlite::params![content],
        )?;
        Ok(())
    }

    fn insights_summary(&self) -> anyhow::Result<InsightsSummary> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("DB Mutex poisoned"))?;
        
        // 1. Total words
        let total_words: Option<i64> = conn.query_row(
            "SELECT SUM(word_count) FROM history WHERE mode = 'dictation'",
            [],
            |row| row.get(0),
        )?;
        let total_words = total_words.unwrap_or(0);
        
        // 2. Average WPM
        let dictation_stats: Option<(i64, i64)> = conn.query_row(
            "SELECT SUM(word_count), SUM(duration_ms) FROM history WHERE mode = 'dictation'",
            [],
            |row| {
                let sum_words: Option<i64> = row.get(0)?;
                let sum_duration: Option<i64> = row.get(1)?;
                Ok(match (sum_words, sum_duration) {
                    (Some(w), Some(d)) => Some((w, d)),
                    _ => None,
                })
            },
        )?;
        
        let avg_wpm = if let Some((words, duration_ms)) = dictation_stats {
            if duration_ms > 0 {
                (words as f64) / (duration_ms as f64 / 60000.0)
            } else {
                0.0
            }
        } else {
            0.0
        };
        
        // 3. Top apps
        let mut stmt_top = conn.prepare(
            "SELECT app_name, COUNT(*) as cnt FROM history GROUP BY app_name ORDER BY cnt DESC, app_name ASC LIMIT 5"
        )?;
        let top_apps_rows = stmt_top.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut top_apps = Vec::new();
        for r in top_apps_rows {
            top_apps.push(r?);
        }
        
        // 4. Daily words (last 365 days) — dictation only, consistent with total_words / avg WPM.
        let mut stmt_daily = conn.prepare(
            "SELECT SUBSTR(ts, 1, 10) as day, SUM(word_count) 
             FROM history 
             WHERE mode = 'dictation'
               AND SUBSTR(ts, 1, 10) >= date('now', 'localtime', '-365 days') 
             GROUP BY day 
             ORDER BY day ASC"
        )?;
        let daily_rows = stmt_daily.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut daily_words = Vec::new();
        for r in daily_rows {
            daily_words.push(r?);
        }
        
        Ok(InsightsSummary {
            total_words,
            avg_wpm,
            top_apps,
            daily_words,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vf_core::CleanupLevel;

    #[test]
    fn test_settings_round_trip() {
        let temp_dir = std::env::temp_dir();
        let file_name = format!("test_settings_{}.json", std::process::id());
        let temp_path = temp_dir.join(file_name);
        
        if temp_path.exists() {
            let _ = std::fs::remove_file(&temp_path);
        }

        // Test 1: Load from non-existent file creates default settings
        let mut settings = load_settings(&temp_path).expect("Failed to load settings");
        assert_eq!(settings.version, 1);
        assert!(!settings.general.start_minimized);
        assert_eq!(settings.llm.cleanup_level, CleanupLevel::Medium);
        assert!(temp_path.exists());

        // Test 2: Modify and save
        settings.general.start_minimized = true;
        save_settings(&settings, &temp_path).expect("Failed to save settings");

        let loaded = load_settings(&temp_path).expect("Failed to reload settings");
        assert!(loaded.general.start_minimized);

        // Test 3: Load from partial/missing-field JSON
        let _ = std::fs::remove_file(&temp_path);
        let partial_json = r#"{"llm": {"api_key": "secret_key"}}"#;
        std::fs::write(&temp_path, partial_json).expect("Failed to write partial JSON");

        let loaded_partial = load_settings(&temp_path).expect("Failed to load partial settings");
        assert_eq!(loaded_partial.llm.api_key, "secret_key");
        assert_eq!(loaded_partial.llm.model, "openai/gpt-oss-120b"); // default filled
        assert!(loaded_partial.general.show_error_notifications); // default filled

        // Test 4: Corrupt JSON recovers with defaults
        let _ = std::fs::remove_file(&temp_path);
        std::fs::write(&temp_path, "{not valid json").expect("write corrupt");
        let recovered = load_settings(&temp_path).expect("recover from corrupt");
        assert_eq!(recovered.version, 1);
        assert!(temp_path.with_extension("json.corrupt").exists()
            || path_has_corrupt_backup(&temp_path));

        // Clean up
        let _ = std::fs::remove_file(&temp_path);
        let _ = std::fs::remove_file(temp_path.with_extension("json.corrupt"));
    }

    fn path_has_corrupt_backup(path: &std::path::Path) -> bool {
        path.with_extension("json.corrupt").exists()
    }

    #[test]
    fn test_sqlite_store() {
        let store = SqliteStore::new_in_memory().expect("Failed to create in-memory DB");

        // 1. Scratchpad test
        let initial_scratch = store.scratchpad_get().expect("Failed to get initial scratchpad");
        assert_eq!(initial_scratch, "");

        store.scratchpad_set("hello world").expect("Failed to set scratchpad");
        assert_eq!(store.scratchpad_get().expect("Failed to get scratchpad"), "hello world");

        // 2. Dictionary test
        let initial_dict = store.dictionary_list().expect("Failed to get initial dictionary");
        assert!(initial_dict.is_empty());

        let entry = store.dictionary_add("testword", "manual").expect("Failed to add word");
        assert_eq!(entry.word, "testword");
        assert_eq!(entry.source, "manual");
        assert!(!entry.starred);
        assert_eq!(entry.use_count, 0);

        let list = store.dictionary_list().expect("Failed to list dictionary");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].word, "testword");

        store.dictionary_toggle_star(entry.id).expect("Failed to toggle star");
        let list_starred = store.dictionary_list().expect("Failed to list dictionary after star");
        assert!(list_starred[0].starred);

        store.dictionary_bump_use_count(&["testword".to_string()]).expect("Failed to bump use count");
        let list_bumped = store.dictionary_list().expect("Failed to list dictionary after bump");
        assert_eq!(list_bumped[0].use_count, 1);

        // Test update
        store.dictionary_update(entry.id, "newword").expect("Failed to update word");
        let list_updated = store.dictionary_list().expect("Failed to list dictionary after update");
        assert_eq!(list_updated[0].word, "newword");

        store.dictionary_delete(entry.id).expect("Failed to delete word");
        assert!(store.dictionary_list().expect("Failed to list after delete").is_empty());

        // 3. History & Insights test — relative local dates so the 365-day window stays valid.
        let today = local_date_offset(0);
        let yesterday = local_date_offset(-1);

        let h1 = HistoryEntry {
            id: None,
            ts: format!("{yesterday}T12:00:00"),
            app_name: "notepad.exe".to_string(),
            window_title: "Untitled - Notepad".to_string(),
            mode: "dictation".to_string(),
            raw_transcript: "hello".to_string(),
            final_text: "hello".to_string(),
            duration_ms: 1000,
            word_count: 1,
        };
        store.history_append(&h1).expect("Failed to append h1");

        let h2 = HistoryEntry {
            id: None,
            ts: format!("{today}T13:00:00"),
            app_name: "chrome.exe".to_string(),
            window_title: "Google".to_string(),
            mode: "dictation".to_string(),
            raw_transcript: "world wide web".to_string(),
            final_text: "World Wide Web".to_string(),
            duration_ms: 2000,
            word_count: 3,
        };
        store.history_append(&h2).expect("Failed to append h2");

        let h_cmd = HistoryEntry {
            id: None,
            ts: format!("{today}T14:00:00"),
            app_name: "chrome.exe".to_string(),
            window_title: "Google".to_string(),
            mode: "command".to_string(),
            raw_transcript: "make it uppercase".to_string(),
            final_text: "MAKE IT UPPERCASE".to_string(),
            duration_ms: 5000,
            word_count: 3,
        };
        store.history_append(&h_cmd).expect("Failed to append h_cmd");

        let history = store.history_list(10, 0).expect("Failed to list history");
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].app_name, "chrome.exe"); // Ordered DESC by ts
        assert_eq!(history[0].mode, "command");
        assert_eq!(history[1].app_name, "chrome.exe");
        assert_eq!(history[1].mode, "dictation");

        let insights = store.insights_summary().expect("Failed to get insights");
        // total_words = 1 + 3 = 4 (command excluded)
        assert_eq!(insights.total_words, 4);
        // avg_wpm:
        // dictation rows word count = 1 + 3 = 4
        // dictation rows duration = 1000 + 2000 = 3000 ms = 0.05 minutes
        // avg_wpm = 4 / 0.05 = 80.0
        assert_eq!(insights.avg_wpm, 80.0);

        // top_apps: chrome.exe (2 rows), notepad.exe (1 row)
        assert_eq!(insights.top_apps.len(), 2);
        assert_eq!(insights.top_apps[0], ("chrome.exe".to_string(), 2));
        assert_eq!(insights.top_apps[1], ("notepad.exe".to_string(), 1));

        // daily_words: dictation only
        // yesterday: 1 word, today: 3 words (command excluded)
        assert_eq!(insights.daily_words.len(), 2);
        assert_eq!(insights.daily_words[0], (yesterday, 1));
        assert_eq!(insights.daily_words[1], (today, 3));
    }

    /// Local calendar date as `YYYY-MM-DD`, offset by whole days from today.
    fn local_date_offset(days: i64) -> String {
        let conn = rusqlite::Connection::open_in_memory().expect("mem");
        conn.query_row(
            "SELECT date('now', 'localtime', ?)",
            rusqlite::params![format!("{days} days")],
            |row| row.get(0),
        )
        .expect("date")
    }
}
