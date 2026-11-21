//! Lokale Daten in einer SQLite-Datei: Konten, Abos, Profile, Playlists, Verlauf, Einstellungen.

use anyhow::Result;
use nrztube_types::*;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::Mutex;

pub struct Db {
    conn: Mutex<Connection>,
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS users (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  username TEXT NOT NULL UNIQUE COLLATE NOCASE,
  password TEXT NOT NULL,
  admin INTEGER NOT NULL DEFAULT 0,
  created INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS sessions (
  token TEXT PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  created INTEGER NOT NULL,
  seen INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS app_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS settings (
  user_id INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
  json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS subscriptions (
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  channel_id TEXT NOT NULL,
  name TEXT NOT NULL,
  thumb TEXT NOT NULL DEFAULT '',
  added INTEGER NOT NULL,
  PRIMARY KEY (user_id, channel_id)
);
CREATE TABLE IF NOT EXISTS profiles (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  color TEXT NOT NULL DEFAULT '',
  position INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS profile_channels (
  profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
  channel_id TEXT NOT NULL,
  PRIMARY KEY (profile_id, channel_id)
);
CREATE TABLE IF NOT EXISTS playlists (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  title TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  protected INTEGER NOT NULL DEFAULT 0,
  created INTEGER NOT NULL,
  updated INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS playlist_videos (
  playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
  video_id TEXT NOT NULL,
  json TEXT NOT NULL,
  position INTEGER NOT NULL,
  added INTEGER NOT NULL,
  PRIMARY KEY (playlist_id, video_id)
);
CREATE TABLE IF NOT EXISTS history (
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  video_id TEXT NOT NULL,
  json TEXT NOT NULL,
  title TEXT NOT NULL DEFAULT '',
  author TEXT NOT NULL DEFAULT '',
  watched INTEGER NOT NULL,
  progress REAL NOT NULL DEFAULT 0,
  PRIMARY KEY (user_id, video_id)
);
CREATE INDEX IF NOT EXISTS history_watched ON history(user_id, watched DESC);
CREATE TABLE IF NOT EXISTS ratings (
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  video_id TEXT NOT NULL,
  rating INTEGER NOT NULL,
  channel_id TEXT NOT NULL DEFAULT '',
  json TEXT NOT NULL,
  at INTEGER NOT NULL,
  PRIMARY KEY (user_id, video_id)
);
CREATE TABLE IF NOT EXISTS searches (
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  q TEXT NOT NULL COLLATE NOCASE,
  count INTEGER NOT NULL DEFAULT 1,
  at INTEGER NOT NULL,
  PRIMARY KEY (user_id, q)
);
CREATE TABLE IF NOT EXISTS twitch_follows (
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  login TEXT NOT NULL,
  name TEXT NOT NULL DEFAULT '',
  thumb TEXT NOT NULL DEFAULT '',
  added INTEGER NOT NULL,
  PRIMARY KEY (user_id, login)
);
-- Gefolgte Konten auf TikTok, Instagram und X
CREATE TABLE IF NOT EXISTS social_follows (
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  platform TEXT NOT NULL,
  handle TEXT NOT NULL,
  name TEXT NOT NULL DEFAULT '',
  avatar TEXT NOT NULL DEFAULT '',
  added INTEGER NOT NULL,
  PRIMARY KEY (user_id, platform, handle)
);
-- YouTube <-> Twitch (für alle Konten gleich; rejected = von Hand gelöst)
CREATE TABLE IF NOT EXISTS channel_links (
  yt_id TEXT NOT NULL,
  tw_login TEXT NOT NULL,
  source TEXT NOT NULL,
  yt_name TEXT NOT NULL DEFAULT '',
  yt_thumb TEXT NOT NULL DEFAULT '',
  tw_name TEXT NOT NULL DEFAULT '',
  tw_thumb TEXT NOT NULL DEFAULT '',
  rejected INTEGER NOT NULL DEFAULT 0,
  at INTEGER NOT NULL,
  PRIMARY KEY (yt_id, tw_login)
);
CREATE INDEX IF NOT EXISTS channel_links_tw ON channel_links(tw_login);
-- Verbundene Konten pro Nutzer: "twitch" (OAuth-Token als JSON), "youtube" (cookies.txt)
CREATE TABLE IF NOT EXISTS accounts (
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  provider TEXT NOT NULL,
  data TEXT NOT NULL,
  updated INTEGER NOT NULL,
  PRIMARY KEY (user_id, provider)
);
-- Einladungen zum Registrieren (einmalig, 7 Tage gültig)
CREATE TABLE IF NOT EXISTS invites (
  code TEXT PRIMARY KEY,
  created_by INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  created INTEGER NOT NULL,
  expires INTEGER NOT NULL,
  used_by TEXT
);
-- Sehzeit pro Video, Tag und Stunde (für den Recap)
CREATE TABLE IF NOT EXISTS watch_log (
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  video_id TEXT NOT NULL,
  channel_id TEXT NOT NULL DEFAULT '',
  day TEXT NOT NULL,
  hour INTEGER NOT NULL,
  seconds REAL NOT NULL DEFAULT 0,
  PRIMARY KEY (user_id, video_id, day, hour)
);
CREATE INDEX IF NOT EXISTS watch_log_user_day ON watch_log(user_id, day);
-- Wann ein Kanal zuletzt auf Verknüpfungen geprüft wurde ("yt:UC…" / "tw:login")
CREATE TABLE IF NOT EXISTS link_checks (key TEXT PRIMARY KEY, at INTEGER NOT NULL);
"#;

/// Spalten, die nach 1.0.0 dazukamen (für die Empfehlungen: wie lange und wie oft geschaut)
const MIGRATIONS: &[&str] = &[
    "ALTER TABLE history ADD COLUMN channel_id TEXT NOT NULL DEFAULT ''",
    "ALTER TABLE history ADD COLUMN duration REAL NOT NULL DEFAULT 0",
    "ALTER TABLE history ADD COLUMN watch_seconds REAL NOT NULL DEFAULT 0",
    "ALTER TABLE history ADD COLUMN opens INTEGER NOT NULL DEFAULT 0",
];

pub const STREAM_LIST: &str = "Stream-Liste";
pub const FAVORITES: &str = "Favoriten";

/// Eine Zeile Verlauf mit allem, was die Empfehlungen brauchen
pub struct WatchRow {
    pub video: VideoItem,
    pub watched: i64,
    pub progress: f64,
    pub duration: f64,
    pub watch_seconds: f64,
    pub opens: i64,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;")?;
        conn.execute_batch(SCHEMA)?;
        for m in MIGRATIONS {
            if let Err(e) = conn.execute(m, []) {
                if !e.to_string().contains("duplicate column") {
                    return Err(e.into());
                }
            }
        }
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn c(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ------------------------------------------------------------ Konten

    pub fn user_count(&self) -> i64 {
        self.c().query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0)).unwrap_or(0)
    }

    pub fn app_setting(&self, key: &str) -> Option<String> {
        self.c()
            .query_row("SELECT value FROM app_settings WHERE key=?", [key], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }

    pub fn set_app_setting(&self, key: &str, value: &str) -> Result<()> {
        self.c().execute(
            "INSERT INTO app_settings(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn create_user(&self, username: &str, hash: &str, admin: bool) -> Result<i64> {
        let c = self.c();
        c.execute(
            "INSERT INTO users(username,password,admin,created) VALUES(?,?,?,?)",
            params![username, hash, admin as i64, now()],
        )?;
        let id = c.last_insert_rowid();
        // Standard-Playlists wie bei FreeTube
        for (title, prot) in [(FAVORITES, 1), ("Später ansehen", 1), (STREAM_LIST, 1)] {
            c.execute(
                "INSERT INTO playlists(user_id,title,protected,created,updated) VALUES(?,?,?,?,?)",
                params![id, title, prot, now(), now()],
            )?;
        }
        Ok(id)
    }

    pub fn user_by_name(&self, username: &str) -> Option<(i64, String, bool)> {
        self.c()
            .query_row("SELECT id,password,admin FROM users WHERE username=?", [username], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0))
            })
            .optional()
            .ok()
            .flatten()
    }

    pub fn users(&self) -> Vec<UserInfo> {
        let c = self.c();
        let mut st = c.prepare("SELECT id,username,admin,created FROM users ORDER BY id").unwrap();
        st.query_map([], |r| {
            Ok(UserInfo { id: r.get(0)?, username: r.get(1)?, admin: r.get::<_, i64>(2)? != 0, created: r.get(3)? })
        })
        .map(|it| it.flatten().collect())
        .unwrap_or_default()
    }

    pub fn delete_user(&self, id: i64) -> Result<()> {
        self.c().execute("DELETE FROM users WHERE id=?", [id])?;
        Ok(())
    }

    pub fn set_password(&self, id: i64, hash: &str) -> Result<()> {
        self.c().execute("UPDATE users SET password=? WHERE id=?", params![hash, id])?;
        self.c().execute("DELETE FROM sessions WHERE user_id=?", [id])?;
        Ok(())
    }

    pub fn create_session(&self, token: &str, user: i64) -> Result<()> {
        self.c().execute(
            "INSERT INTO sessions(token,user_id,created,seen) VALUES(?,?,?,?)",
            params![token, user, now(), now()],
        )?;
        Ok(())
    }

    pub fn session_user(&self, token: &str) -> Option<Me> {
        let c = self.c();
        let me = c
            .query_row(
                "SELECT u.id,u.username,u.admin,s.seen FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.token=?",
                [token],
                |r| Ok((Me { id: r.get(0)?, username: r.get(1)?, admin: r.get::<_, i64>(2)? != 0 }, r.get::<_, i64>(3)?)),
            )
            .optional()
            .ok()
            .flatten()?;
        // 90 Tage ohne Besuch -> abgelaufen
        if now() - me.1 > 90 * 86400 {
            let _ = c.execute("DELETE FROM sessions WHERE token=?", [token]);
            return None;
        }
        if now() - me.1 > 3600 {
            let _ = c.execute("UPDATE sessions SET seen=? WHERE token=?", params![now(), token]);
        }
        Some(me.0)
    }

    pub fn delete_session(&self, token: &str) {
        let _ = self.c().execute("DELETE FROM sessions WHERE token=?", [token]);
    }

    // ------------------------------------------------------------ Einstellungen

    pub fn settings(&self, user: i64) -> Settings {
        self.c()
            .query_row("SELECT json FROM settings WHERE user_id=?", [user], |r| r.get::<_, String>(0))
            .optional()
            .ok()
            .flatten()
            .and_then(|j| {
                let mut s: Settings = serde_json::from_str(&j).ok()?;
                // Vor 1.1.0 gab es keine Startseite mit Vorschlägen: einmal darauf umstellen
                if !j.contains("\"reco_enabled\"") && s.start_page == "feed" {
                    s.start_page = "home".into();
                }
                Some(s)
            })
            .unwrap_or_default()
    }

    pub fn save_settings(&self, user: i64, s: &Settings) -> Result<()> {
        self.c().execute(
            "INSERT INTO settings(user_id,json) VALUES(?,?) ON CONFLICT(user_id) DO UPDATE SET json=excluded.json",
            params![user, serde_json::to_string(s)?],
        )?;
        Ok(())
    }

    // ------------------------------------------------------------ Abos

    pub fn subscriptions(&self, user: i64) -> Vec<Subscription> {
        let c = self.c();
        let mut st = c
            .prepare("SELECT channel_id,name,thumb,added FROM subscriptions WHERE user_id=? ORDER BY name COLLATE NOCASE")
            .unwrap();
        st.query_map([user], |r| Ok(Subscription { channel_id: r.get(0)?, name: r.get(1)?, thumb: r.get(2)?, added: r.get(3)? }))
            .map(|it| it.flatten().collect())
            .unwrap_or_default()
    }

    /// true, wenn neu
    pub fn subscribe(&self, user: i64, s: &Subscription) -> Result<bool> {
        let n = self.c().execute(
            "INSERT INTO subscriptions(user_id,channel_id,name,thumb,added) VALUES(?,?,?,?,?)
             ON CONFLICT(user_id,channel_id) DO UPDATE SET name=CASE WHEN excluded.name<>'' THEN excluded.name ELSE name END,
             thumb=CASE WHEN excluded.thumb<>'' THEN excluded.thumb ELSE thumb END",
            params![user, s.channel_id, s.name, s.thumb, if s.added > 0 { s.added } else { now() }],
        )?;
        Ok(n > 0)
    }

    pub fn is_subscribed(&self, user: i64, channel: &str) -> bool {
        self.c()
            .query_row("SELECT 1 FROM subscriptions WHERE user_id=? AND channel_id=?", params![user, channel], |_| Ok(()))
            .optional()
            .ok()
            .flatten()
            .is_some()
    }

    pub fn unsubscribe(&self, user: i64, channel: &str) -> Result<()> {
        let c = self.c();
        c.execute("DELETE FROM subscriptions WHERE user_id=? AND channel_id=?", params![user, channel])?;
        c.execute(
            "DELETE FROM profile_channels WHERE channel_id=? AND profile_id IN (SELECT id FROM profiles WHERE user_id=?)",
            params![channel, user],
        )?;
        Ok(())
    }

    /// Alle abonnierten Kanäle aller Konten (für "Beliebt" und das Vorladen der Feeds)
    pub fn all_subscribed_channels(&self) -> Vec<String> {
        let c = self.c();
        let mut st = c.prepare("SELECT DISTINCT channel_id FROM subscriptions").unwrap();
        st.query_map([], |r| r.get(0)).map(|it| it.flatten().collect()).unwrap_or_default()
    }

    // ------------------------------------------------------------ Profile

    pub fn profiles(&self, user: i64) -> Vec<Profile> {
        let c = self.c();
        let mut st = c.prepare("SELECT id,name,color FROM profiles WHERE user_id=? ORDER BY position,id").unwrap();
        let mut list: Vec<Profile> = st
            .query_map([user], |r| Ok(Profile { id: r.get(0)?, name: r.get(1)?, color: r.get(2)?, channels: vec![] }))
            .map(|it| it.flatten().collect())
            .unwrap_or_default();
        let mut st = c.prepare("SELECT channel_id FROM profile_channels WHERE profile_id=?").unwrap();
        for p in list.iter_mut() {
            p.channels = st.query_map([p.id], |r| r.get(0)).map(|it| it.flatten().collect()).unwrap_or_default();
        }
        list
    }

    pub fn save_profile(&self, user: i64, p: &Profile) -> Result<i64> {
        let c = self.c();
        let id = if p.id > 0 {
            let n = c.execute(
                "UPDATE profiles SET name=?,color=? WHERE id=? AND user_id=?",
                params![p.name, p.color, p.id, user],
            )?;
            if n == 0 {
                anyhow::bail!("Profil nicht gefunden");
            }
            p.id
        } else {
            c.execute(
                "INSERT INTO profiles(user_id,name,color,position) VALUES(?,?,?,(SELECT COALESCE(MAX(position),0)+1 FROM profiles WHERE user_id=?))",
                params![user, p.name, p.color, user],
            )?;
            c.last_insert_rowid()
        };
        c.execute("DELETE FROM profile_channels WHERE profile_id=?", [id])?;
        for ch in &p.channels {
            c.execute("INSERT OR IGNORE INTO profile_channels(profile_id,channel_id) VALUES(?,?)", params![id, ch])?;
        }
        Ok(id)
    }

    pub fn delete_profile(&self, user: i64, id: i64) -> Result<()> {
        self.c().execute("DELETE FROM profiles WHERE id=? AND user_id=?", params![id, user])?;
        Ok(())
    }

    // ------------------------------------------------------------ Playlists

    pub fn playlists(&self, user: i64) -> Vec<LocalPlaylist> {
        let c = self.c();
        let mut st = c
            .prepare(
                "SELECT p.id,p.title,p.description,p.protected,p.updated,
                 (SELECT COUNT(*) FROM playlist_videos v WHERE v.playlist_id=p.id),
                 (SELECT video_id FROM playlist_videos v WHERE v.playlist_id=p.id ORDER BY position LIMIT 1)
                 FROM playlists p WHERE user_id=? ORDER BY protected DESC, updated DESC",
            )
            .unwrap();
        st.query_map([user], |r| {
            let first: Option<String> = r.get(6)?;
            Ok(LocalPlaylist {
                id: r.get(0)?,
                title: r.get(1)?,
                description: r.get(2)?,
                protected: r.get::<_, i64>(3)? != 0,
                updated: r.get(4)?,
                count: r.get::<_, i64>(5)? as u32,
                thumb: first.map(|id| crate::proxy::video_thumb(&id)).unwrap_or_default(),
                videos: vec![],
            })
        })
        .map(|it| it.flatten().collect())
        .unwrap_or_default()
    }

    pub fn playlist(&self, user: i64, id: i64) -> Option<LocalPlaylist> {
        let mut p = self.playlists(user).into_iter().find(|p| p.id == id)?;
        let c = self.c();
        let mut st = c.prepare("SELECT json FROM playlist_videos WHERE playlist_id=? ORDER BY position").unwrap();
        p.videos = st
            .query_map([id], |r| r.get::<_, String>(0))
            .map(|it| it.flatten().filter_map(|j| serde_json::from_str(&j).ok()).collect())
            .unwrap_or_default();
        Some(p)
    }

    fn owns_playlist(&self, user: i64, id: i64) -> bool {
        self.c()
            .query_row("SELECT 1 FROM playlists WHERE id=? AND user_id=?", params![id, user], |_| Ok(()))
            .optional()
            .ok()
            .flatten()
            .is_some()
    }

    pub fn create_playlist(&self, user: i64, title: &str, description: &str) -> Result<i64> {
        let c = self.c();
        c.execute(
            "INSERT INTO playlists(user_id,title,description,created,updated) VALUES(?,?,?,?,?)",
            params![user, title, description, now(), now()],
        )?;
        Ok(c.last_insert_rowid())
    }

    pub fn update_playlist(&self, user: i64, id: i64, title: &str, description: &str) -> Result<()> {
        self.c().execute(
            "UPDATE playlists SET title=?,description=?,updated=? WHERE id=? AND user_id=? AND protected=0",
            params![title, description, now(), id, user],
        )?;
        Ok(())
    }

    pub fn delete_playlist(&self, user: i64, id: i64) -> Result<()> {
        self.c().execute("DELETE FROM playlists WHERE id=? AND user_id=? AND protected=0", params![id, user])?;
        Ok(())
    }

    pub fn add_to_playlist(&self, user: i64, id: i64, v: &VideoItem) -> Result<bool> {
        if !self.owns_playlist(user, id) {
            anyhow::bail!("Playlist nicht gefunden");
        }
        let c = self.c();
        let n = c.execute(
            "INSERT OR IGNORE INTO playlist_videos(playlist_id,video_id,json,position,added)
             VALUES(?,?,?,(SELECT COALESCE(MAX(position),0)+1 FROM playlist_videos WHERE playlist_id=?),?)",
            params![id, v.id, serde_json::to_string(v)?, id, now()],
        )?;
        c.execute("UPDATE playlists SET updated=? WHERE id=?", params![now(), id])?;
        Ok(n > 0)
    }

    pub fn remove_from_playlist(&self, user: i64, id: i64, video: &str) -> Result<()> {
        if !self.owns_playlist(user, id) {
            anyhow::bail!("Playlist nicht gefunden");
        }
        self.c().execute("DELETE FROM playlist_videos WHERE playlist_id=? AND video_id=?", params![id, video])?;
        Ok(())
    }

    pub fn move_in_playlist(&self, user: i64, id: i64, video: &str, delta: i64) -> Result<()> {
        let Some(p) = self.playlist(user, id) else { anyhow::bail!("Playlist nicht gefunden") };
        let mut ids: Vec<String> = p.videos.iter().map(|v| v.id.clone()).collect();
        let Some(i) = ids.iter().position(|x| x == video) else { return Ok(()) };
        let j = (i as i64 + delta).clamp(0, ids.len() as i64 - 1) as usize;
        let item = ids.remove(i);
        ids.insert(j, item);
        let c = self.c();
        for (pos, vid) in ids.iter().enumerate() {
            c.execute("UPDATE playlist_videos SET position=? WHERE playlist_id=? AND video_id=?", params![pos as i64, id, vid])?;
        }
        Ok(())
    }

    /// In welchen Playlists des Kontos steckt das Video?
    pub fn playlists_with(&self, user: i64, video: &str) -> Vec<i64> {
        let c = self.c();
        let mut st = c
            .prepare("SELECT v.playlist_id FROM playlist_videos v JOIN playlists p ON p.id=v.playlist_id WHERE p.user_id=? AND v.video_id=?")
            .unwrap();
        st.query_map(params![user, video], |r| r.get(0)).map(|it| it.flatten().collect()).unwrap_or_default()
    }

    // ------------------------------------------------------------ Verlauf

    pub fn history(&self, user: i64, q: &str, limit: i64, offset: i64) -> Vec<HistoryEntry> {
        let c = self.c();
        let like = format!("%{}%", q.replace('%', ""));
        let mut st = c
            .prepare(
                "SELECT json,watched,progress FROM history WHERE user_id=? AND (?='' OR title LIKE ? OR author LIKE ?)
                 ORDER BY watched DESC LIMIT ? OFFSET ?",
            )
            .unwrap();
        st.query_map(params![user, q, like, like, limit, offset], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, f64>(2)?))
        })
        .map(|it| {
            it.flatten()
                .filter_map(|(j, w, p)| Some(HistoryEntry { video: serde_json::from_str(&j).ok()?, watched: w, progress: p }))
                .collect()
        })
        .unwrap_or_default()
    }

    pub fn history_entry(&self, user: i64, video: &str) -> Option<HistoryEntry> {
        self.c()
            .query_row(
                "SELECT json,watched,progress FROM history WHERE user_id=? AND video_id=?",
                params![user, video],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, f64>(2)?)),
            )
            .optional()
            .ok()
            .flatten()
            .and_then(|(j, w, p)| Some(HistoryEntry { video: serde_json::from_str(&j).ok()?, watched: w, progress: p }))
    }

    pub fn watched_ids(&self, user: i64) -> std::collections::HashSet<String> {
        let c = self.c();
        let mut st = c.prepare("SELECT video_id FROM history WHERE user_id=?").unwrap();
        st.query_map([user], |r| r.get(0)).map(|it| it.flatten().collect()).unwrap_or_default()
    }

    pub fn add_history(&self, user: i64, v: &VideoItem, progress: f64, watched: Option<i64>) -> Result<()> {
        self.record_watch(user, v, progress, watched, 0.0, 0.0, false)
    }

    /// Verlauf schreiben und Sehdauer aufsummieren
    #[allow(clippy::too_many_arguments)]
    pub fn record_watch(&self, user: i64, v: &VideoItem, progress: f64, watched: Option<i64>, played: f64, duration: f64, opened: bool) -> Result<()> {
        let duration = if duration > 0.0 { duration } else { v.duration.unwrap_or(0) as f64 };
        self.c().execute(
            "INSERT INTO history(user_id,video_id,json,title,author,watched,progress,channel_id,duration,watch_seconds,opens)
             VALUES(?,?,?,?,?,?,?,?,?,?,?)
             ON CONFLICT(user_id,video_id) DO UPDATE SET json=excluded.json,title=excluded.title,author=excluded.author,
             watched=excluded.watched,progress=excluded.progress,
             channel_id=CASE WHEN excluded.channel_id<>'' THEN excluded.channel_id ELSE history.channel_id END,
             duration=MAX(history.duration,excluded.duration),
             watch_seconds=history.watch_seconds+excluded.watch_seconds,
             opens=history.opens+excluded.opens",
            params![
                user,
                v.id,
                serde_json::to_string(v)?,
                v.title,
                v.author,
                watched.unwrap_or_else(now),
                progress,
                v.author_id,
                duration,
                played.clamp(0.0, 120.0),
                opened as i64
            ],
        )?;
        // Tagesprotokoll für den Recap (Ortszeit)
        let played = played.clamp(0.0, 120.0);
        if played > 0.0 {
            let t = chrono::Local::now();
            self.c().execute(
                "INSERT INTO watch_log(user_id,video_id,channel_id,day,hour,seconds) VALUES(?,?,?,?,?,?)
                 ON CONFLICT(user_id,video_id,day,hour) DO UPDATE SET seconds=watch_log.seconds+excluded.seconds,
                 channel_id=CASE WHEN excluded.channel_id<>'' THEN excluded.channel_id ELSE watch_log.channel_id END",
                params![user, v.id, v.author_id, t.format("%Y-%m-%d").to_string(), t.format("%H").to_string().parse::<i64>().unwrap_or(0), played],
            )?;
        }
        Ok(())
    }

    pub fn watch_rows(&self, user: i64, limit: i64) -> Vec<WatchRow> {
        let c = self.c();
        let mut st = c
            .prepare(
                "SELECT json,watched,progress,duration,watch_seconds,opens FROM history WHERE user_id=?
                 ORDER BY watched DESC LIMIT ?",
            )
            .unwrap();
        st.query_map(params![user, limit], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, f64>(2)?,
                r.get::<_, f64>(3)?,
                r.get::<_, f64>(4)?,
                r.get::<_, i64>(5)?,
            ))
        })
        .map(|it| {
            it.flatten()
                .filter_map(|(j, watched, progress, duration, watch_seconds, opens)| {
                    Some(WatchRow { video: serde_json::from_str(&j).ok()?, watched, progress, duration, watch_seconds, opens })
                })
                .collect()
        })
        .unwrap_or_default()
    }

    /// Empfehlungsdaten zurücksetzen (Sehdauer, Bewertungen ohne Favoriten, Suchen)
    pub fn reset_taste(&self, user: i64) -> Result<()> {
        let c = self.c();
        c.execute("UPDATE history SET watch_seconds=0, opens=0 WHERE user_id=?", [user])?;
        c.execute("DELETE FROM ratings WHERE user_id=? AND rating<0", [user])?;
        c.execute("DELETE FROM searches WHERE user_id=?", [user])?;
        Ok(())
    }

    // ------------------------------------------------------------ Bewertungen

    /// ID einer geschützten Playlist (legt sie an, falls ein älteres Konto sie noch nicht hat)
    pub fn special_playlist(&self, user: i64, title: &str) -> Result<i64> {
        let c = self.c();
        if let Some(id) = c
            .query_row("SELECT id FROM playlists WHERE user_id=? AND protected=1 AND title=?", params![user, title], |r| r.get(0))
            .optional()?
        {
            return Ok(id);
        }
        c.execute(
            "INSERT INTO playlists(user_id,title,protected,created,updated) VALUES(?,?,1,?,?)",
            params![user, title, now(), now()],
        )?;
        Ok(c.last_insert_rowid())
    }

    pub fn rating(&self, user: i64, video: &str) -> i8 {
        if let Ok(fav) = self.special_playlist(user, FAVORITES) {
            let liked: bool = self
                .c()
                .query_row("SELECT 1 FROM playlist_videos WHERE playlist_id=? AND video_id=?", params![fav, video], |_| Ok(true))
                .optional()
                .ok()
                .flatten()
                .unwrap_or(false);
            if liked {
                return 1;
            }
        }
        self.c()
            .query_row("SELECT rating FROM ratings WHERE user_id=? AND video_id=? AND rating<0", params![user, video], |r| r.get::<_, i64>(0))
            .optional()
            .ok()
            .flatten()
            .map(|r| r as i8)
            .unwrap_or(0)
    }

    /// Gefällt mir = in die Favoriten; Gefällt mir nicht = merken und aus den Favoriten nehmen
    pub fn rate(&self, user: i64, v: &VideoItem, rating: i8) -> Result<()> {
        let fav = self.special_playlist(user, FAVORITES)?;
        if rating > 0 {
            self.add_to_playlist(user, fav, v)?;
        } else {
            self.remove_from_playlist(user, fav, &v.id)?;
        }
        let c = self.c();
        if rating == 0 {
            c.execute("DELETE FROM ratings WHERE user_id=? AND video_id=?", params![user, v.id])?;
        } else {
            c.execute(
                "INSERT INTO ratings(user_id,video_id,rating,channel_id,json,at) VALUES(?,?,?,?,?,?)
                 ON CONFLICT(user_id,video_id) DO UPDATE SET rating=excluded.rating,channel_id=excluded.channel_id,json=excluded.json,at=excluded.at",
                params![user, v.id, rating as i64, v.author_id, serde_json::to_string(v)?, now()],
            )?;
        }
        Ok(())
    }

    /// Alle Bewertungen: Favoriten zählen als "Gefällt mir"
    pub fn ratings(&self, user: i64) -> Vec<(VideoItem, i8, i64)> {
        let mut out: Vec<(VideoItem, i8, i64)> = Vec::new();
        let fav = self.special_playlist(user, FAVORITES).unwrap_or(0);
        let c = self.c();
        if let Ok(mut st) = c.prepare("SELECT json,added FROM playlist_videos WHERE playlist_id=?") {
            if let Ok(it) = st.query_map([fav], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))) {
                for (j, at) in it.flatten() {
                    if let Ok(v) = serde_json::from_str::<VideoItem>(&j) {
                        out.push((v, 1, at));
                    }
                }
            }
        }
        if let Ok(mut st) = c.prepare("SELECT json,at FROM ratings WHERE user_id=? AND rating<0") {
            if let Ok(it) = st.query_map([user], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))) {
                for (j, at) in it.flatten() {
                    if let Ok(v) = serde_json::from_str::<VideoItem>(&j) {
                        out.push((v, -1, at));
                    }
                }
            }
        }
        out
    }

    // ------------------------------------------------------------ Suchen

    pub fn record_search(&self, user: i64, q: &str) -> Result<()> {
        let q = q.trim();
        if q.len() < 2 || q.len() > 120 {
            return Ok(());
        }
        self.c().execute(
            "INSERT INTO searches(user_id,q,count,at) VALUES(?,?,1,?)
             ON CONFLICT(user_id,q) DO UPDATE SET count=searches.count+1, at=excluded.at",
            params![user, q, now()],
        )?;
        Ok(())
    }

    /// Letzte Suchen (neueste zuerst) mit Anzahl
    pub fn searches(&self, user: i64, limit: i64) -> Vec<(String, i64, i64)> {
        let c = self.c();
        let mut st = c.prepare("SELECT q,count,at FROM searches WHERE user_id=? ORDER BY at DESC LIMIT ?").unwrap();
        st.query_map(params![user, limit], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map(|it| it.flatten().collect())
            .unwrap_or_default()
    }

    pub fn delete_history(&self, user: i64, video: Option<&str>) -> Result<()> {
        match video {
            Some(v) => self.c().execute("DELETE FROM history WHERE user_id=? AND video_id=?", params![user, v])?,
            None => {
                self.c().execute("DELETE FROM searches WHERE user_id=?", [user])?;
                self.c().execute("DELETE FROM history WHERE user_id=?", [user])?
            }
        };
        Ok(())
    }

    // ------------------------------------------------------------ Twitch

    pub fn twitch_follows(&self, user: i64) -> Vec<TwitchFollow> {
        let c = self.c();
        let Ok(mut st) = c.prepare("SELECT login,name,thumb,added FROM twitch_follows WHERE user_id=? ORDER BY name COLLATE NOCASE") else { return vec![] };
        st.query_map([user], |r| Ok(TwitchFollow { login: r.get(0)?, name: r.get(1)?, thumb: r.get(2)?, added: r.get(3)? }))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    pub fn twitch_follow(&self, user: i64, f: &TwitchFollow) -> Result<()> {
        self.c().execute(
            "INSERT INTO twitch_follows(user_id,login,name,thumb,added) VALUES(?,?,?,?,?)
             ON CONFLICT(user_id,login) DO UPDATE SET name=excluded.name, thumb=excluded.thumb",
            params![user, f.login.to_lowercase(), f.name, f.thumb, now()],
        )?;
        Ok(())
    }

    pub fn twitch_unfollow(&self, user: i64, login: &str) -> Result<()> {
        self.c().execute("DELETE FROM twitch_follows WHERE user_id=? AND login=?", params![user, login.to_lowercase()])?;
        Ok(())
    }

    // ------------------------------------------------------------ Social

    pub fn social_follows(&self, user: i64) -> Vec<SocialFollow> {
        let c = self.c();
        let Ok(mut st) = c.prepare("SELECT platform,handle,name,avatar,added FROM social_follows WHERE user_id=? ORDER BY platform, name COLLATE NOCASE") else { return vec![] };
        st.query_map([user], |r| Ok(SocialFollow { platform: r.get(0)?, handle: r.get(1)?, name: r.get(2)?, avatar: r.get(3)?, added: r.get(4)? }))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    pub fn social_follow(&self, user: i64, f: &SocialFollow) -> Result<()> {
        self.c().execute(
            "INSERT INTO social_follows(user_id,platform,handle,name,avatar,added) VALUES(?,?,?,?,?,?)
             ON CONFLICT(user_id,platform,handle) DO UPDATE SET name=excluded.name, avatar=excluded.avatar",
            params![user, f.platform, f.handle, f.name, f.avatar, now()],
        )?;
        Ok(())
    }

    pub fn social_unfollow(&self, user: i64, platform: &str, handle: &str) -> Result<()> {
        self.c().execute("DELETE FROM social_follows WHERE user_id=? AND platform=? AND handle=?", params![user, platform, handle])?;
        Ok(())
    }

    /// Verknüpfung speichern. Eine von Hand gelöste bleibt gelöst (ausser bei "manual").
    pub fn save_link(&self, l: &ChannelLink) -> Result<()> {
        let manual = l.source == "manual";
        self.c().execute(
            "INSERT INTO channel_links(yt_id,tw_login,source,yt_name,yt_thumb,tw_name,tw_thumb,rejected,at) VALUES(?,?,?,?,?,?,?,0,?)
             ON CONFLICT(yt_id,tw_login) DO UPDATE SET
               source=CASE WHEN ?9 OR (channel_links.source<>'manual' AND channel_links.rejected=0) THEN excluded.source ELSE channel_links.source END,
               rejected=CASE WHEN ?9 THEN 0 ELSE channel_links.rejected END,
               yt_name=CASE WHEN excluded.yt_name<>'' THEN excluded.yt_name ELSE channel_links.yt_name END,
               yt_thumb=CASE WHEN excluded.yt_thumb<>'' THEN excluded.yt_thumb ELSE channel_links.yt_thumb END,
               tw_name=CASE WHEN excluded.tw_name<>'' THEN excluded.tw_name ELSE channel_links.tw_name END,
               tw_thumb=CASE WHEN excluded.tw_thumb<>'' THEN excluded.tw_thumb ELSE channel_links.tw_thumb END",
            params![l.yt_id, l.tw_login.to_lowercase(), l.source, l.yt_name, l.yt_thumb, l.tw_name, l.tw_thumb, now(), manual],
        )?;
        Ok(())
    }

    pub fn reject_link(&self, yt: &str, tw: &str) -> Result<()> {
        self.c().execute(
            "INSERT INTO channel_links(yt_id,tw_login,source,rejected,at) VALUES(?,?,'manual',1,?)
             ON CONFLICT(yt_id,tw_login) DO UPDATE SET rejected=1",
            params![yt, tw.to_lowercase(), now()],
        )?;
        Ok(())
    }

    fn links_where(&self, cond: &str, key: &str) -> Vec<ChannelLink> {
        let c = self.c();
        let sql = format!("SELECT yt_id,tw_login,source,yt_name,yt_thumb,tw_name,tw_thumb FROM channel_links WHERE rejected=0 AND {cond} ORDER BY source='manual' DESC, at");
        let Ok(mut st) = c.prepare(&sql) else { return vec![] };
        st.query_map([key], |r| {
            Ok(ChannelLink {
                yt_id: r.get(0)?, tw_login: r.get(1)?, source: r.get(2)?, yt_name: r.get(3)?, yt_thumb: r.get(4)?,
                tw_name: r.get(5)?, tw_thumb: r.get(6)?, live: None,
            })
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }

    pub fn links_for_youtube(&self, yt: &str) -> Vec<ChannelLink> {
        self.links_where("yt_id=?", yt)
    }

    pub fn links_for_twitch(&self, tw: &str) -> Vec<ChannelLink> {
        self.links_where("tw_login=?", &tw.to_lowercase())
    }

    pub fn is_rejected(&self, yt: &str, tw: &str) -> bool {
        self.c()
            .query_row("SELECT rejected FROM channel_links WHERE yt_id=? AND tw_login=?", params![yt, tw.to_lowercase()], |r| r.get::<_, i64>(0))
            .map(|x| x == 1)
            .unwrap_or(false)
    }

    /// Alle Twitch-Logins, die mit einem der YouTube-Kanäle verknüpft sind
    pub fn twitch_for_youtube_many(&self, yt_ids: &[String]) -> Vec<(String, String)> {
        let c = self.c();
        let Ok(mut st) = c.prepare("SELECT yt_id,tw_login FROM channel_links WHERE rejected=0") else { return vec![] };
        let set: std::collections::HashSet<&String> = yt_ids.iter().collect();
        st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map(|rows| rows.flatten().filter(|(y, _)| set.contains(y)).collect())
            .unwrap_or_default()
    }

    /// Zuletzt geprüft vor weniger als `max_age` Sekunden?
    pub fn link_checked_recently(&self, key: &str, max_age: i64) -> bool {
        self.c()
            .query_row("SELECT at FROM link_checks WHERE key=?", [key], |r| r.get::<_, i64>(0))
            .map(|at| now() - at < max_age)
            .unwrap_or(false)
    }

    pub fn mark_link_checked(&self, key: &str) {
        let _ = self.c().execute(
            "INSERT INTO link_checks(key,at) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET at=excluded.at",
            params![key, now()],
        );
    }

    // ------------------------------------------------------------ Verbundene Konten

    pub fn account(&self, user: i64, provider: &str) -> Option<String> {
        self.c()
            .query_row("SELECT data FROM accounts WHERE user_id=? AND provider=?", params![user, provider], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }

    pub fn set_account(&self, user: i64, provider: &str, data: &str) -> Result<()> {
        self.c().execute(
            "INSERT INTO accounts(user_id,provider,data,updated) VALUES(?,?,?,?)
             ON CONFLICT(user_id,provider) DO UPDATE SET data=excluded.data, updated=excluded.updated",
            params![user, provider, data, now()],
        )?;
        Ok(())
    }

    pub fn delete_account(&self, user: i64, provider: &str) -> Result<()> {
        self.c().execute("DELETE FROM accounts WHERE user_id=? AND provider=?", params![user, provider])?;
        Ok(())
    }

    // ------------------------------------------------------------ Einladungen

    pub fn create_invite(&self, user: i64, code: &str) -> Result<()> {
        let t = now();
        self.c().execute("INSERT INTO invites(code,created_by,created,expires) VALUES(?,?,?,?)", params![code, user, t, t + 7 * 86400])?;
        Ok(())
    }

    pub fn invites(&self, user: i64) -> Vec<Invite> {
        let c = self.c();
        let Ok(mut st) = c.prepare("SELECT code,created,expires,used_by FROM invites WHERE created_by=? ORDER BY created DESC LIMIT 20") else { return vec![] };
        st.query_map([user], |r| Ok(Invite { code: r.get(0)?, created: r.get(1)?, expires: r.get(2)?, used_by: r.get(3)? }))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    pub fn delete_invite(&self, user: i64, code: &str) -> Result<()> {
        self.c().execute("DELETE FROM invites WHERE code=? AND created_by=? AND used_by IS NULL", params![code, user])?;
        Ok(())
    }

    /// Einladung prüfen und verbrauchen
    pub fn use_invite(&self, code: &str, username: &str) -> bool {
        self.c()
            .execute("UPDATE invites SET used_by=? WHERE code=? AND used_by IS NULL AND expires>?", params![username, code, now()])
            .map(|n| n == 1)
            .unwrap_or(false)
    }

    pub fn invite_valid(&self, code: &str) -> bool {
        self.c()
            .query_row("SELECT COUNT(*) FROM invites WHERE code=? AND used_by IS NULL AND expires>?", params![code, now()], |r| r.get::<_, i64>(0))
            .map(|n| n == 1)
            .unwrap_or(false)
    }

    // ------------------------------------------------------------ Recap

    /// Tagesprotokoll ab `since` (JJJJ-MM-TT, inklusive): (video, kanal, tag, stunde, sekunden)
    pub fn watch_log(&self, user: i64, since: &str) -> Vec<(String, String, String, i64, f64)> {
        let c = self.c();
        let Ok(mut st) = c.prepare("SELECT video_id,channel_id,day,hour,seconds FROM watch_log WHERE user_id=? AND day>=? ORDER BY day") else { return vec![] };
        st.query_map(params![user, since], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    pub fn first_log_day(&self, user: i64) -> Option<String> {
        self.c().query_row("SELECT MIN(day) FROM watch_log WHERE user_id=?", [user], |r| r.get::<_, Option<String>>(0)).ok().flatten()
    }

    /// Verlauf mit Sehzeit: video -> (eintrag, sekunden gesamt, geöffnet, zuletzt, stelle, länge)
    pub fn history_stats(&self, user: i64) -> Vec<(VideoItem, f64, i64, i64, f64, f64)> {
        let c = self.c();
        let Ok(mut st) = c.prepare("SELECT json,watch_seconds,opens,watched,progress,duration,channel_id FROM history WHERE user_id=?") else { return vec![] };
        st.query_map([user], |r| {
            let j: String = r.get(0)?;
            let ch: String = r.get(6)?;
            Ok((j, r.get::<_, f64>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, f64>(4)?, r.get::<_, f64>(5)?, ch))
        })
        .map(|rows| {
            rows.flatten()
                .filter_map(|(j, s, o, w, p, d, ch)| {
                    let mut v: VideoItem = serde_json::from_str(&j).ok()?;
                    if v.author_id.is_empty() {
                        v.author_id = ch;
                    }
                    Some((v, s, o, w, p, d))
                })
                .collect()
        })
        .unwrap_or_default()
    }
}
