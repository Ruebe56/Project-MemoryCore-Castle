//! HOME's FEEDS card: small status lists the companion (or any script) keeps up to date.
//!
//! Every `<memory folder>/feeds/*.json` is one feed:
//!
//! ```json
//! { "title": "SENSES", "updated": "2026-11-16T10:20:00+08:00",
//!   "rows": [ { "label": "Voice", "state": "on", "detail": "port 8760" } ] }
//! ```
//!
//! `state` is `on`, `off`, `warn` or anything else (shown as plain). Castle only reads these
//! files; it never runs anything they name. Sizes are capped so a runaway file cannot flood HOME.
//!
//! A row may also carry `"id"` and `"switch": "on" | "off" | "busy"`: HOME then shows a small
//! switch. Pressing it only writes a request file to `<memory folder>/feeds/inbox/`
//! (`{"feed","id","action":"start"|"stop","at"}`); whoever writes the feed reads that inbox and
//! decides what to do. Castle still runs nothing, and it writes a request only for a row the
//! feed itself offered a switch on.

use serde::Serialize;
use serde_json::Value;
use std::path::Path;

const MAX_FEEDS: usize = 6;
const MAX_ROWS: usize = 16;
const MAX_BYTES: u64 = 64 * 1024;
const MAX_TEXT: usize = 80;
const MAX_ID: usize = 32;
const MAX_INBOX: usize = 20;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FeedRow {
    pub label: String,
    pub state: String,
    pub detail: String,
    /// the row's id, only when it is a safe name (a-z, 0-9, `-`, `_`)
    pub id: String,
    /// `on`, `off` or `busy` when the row offers a switch, else None
    pub switch: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Feed {
    /// the file name without `.json`
    pub name: String,
    pub title: String,
    /// as the file gave it (ISO 8601), or empty
    pub updated: String,
    pub rows: Vec<FeedRow>,
}

fn text(v: Option<&Value>) -> String {
    let s = match v {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    };
    if s.chars().count() > MAX_TEXT {
        let cut: String = s.chars().take(MAX_TEXT - 1).collect();
        format!("{cut}…")
    } else {
        s
    }
}

/// A name safe to put in a file name: a-z, 0-9, `-` and `_`, at most 32 characters.
pub fn safe_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_ID && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

fn switch(v: Option<&Value>, id: &str) -> Option<String> {
    if !safe_name(id) {
        return None;
    }
    let s = text(v).to_lowercase();
    ["on", "off", "busy"].contains(&s.as_str()).then_some(s)
}

fn state(v: Option<&Value>) -> String {
    match v {
        Some(Value::Bool(true)) => "on".into(),
        Some(Value::Bool(false)) => "off".into(),
        _ => {
            let s = text(v).to_lowercase();
            if ["on", "off", "warn"].contains(&s.as_str()) { s } else { "plain".into() }
        }
    }
}

/// One feed from the file's text, or None when it is not a feed.
pub fn parse(name: &str, raw: &str) -> Option<Feed> {
    let v: Value = serde_json::from_str(raw).ok()?;
    let obj = v.as_object()?;
    let rows = obj.get("rows")?.as_array()?;
    let rows: Vec<FeedRow> = rows
        .iter()
        .filter_map(|r| r.as_object())
        .map(|r| {
            let id = text(r.get("id"));
            let switch = switch(r.get("switch"), &id);
            FeedRow {
                label: text(r.get("label")),
                state: state(r.get("state")),
                detail: text(r.get("detail")),
                id: if safe_name(&id) { id } else { String::new() },
                switch,
            }
        })
        .filter(|r| !r.label.is_empty())
        .take(MAX_ROWS)
        .collect();
    let title = text(obj.get("title"));
    Some(Feed {
        name: name.to_string(),
        title: if title.is_empty() { name.to_uppercase() } else { title },
        updated: text(obj.get("updated")),
        rows,
    })
}

/// Every feed in `<folder>/feeds/`, by file name.
pub fn read_feeds(folder: &Path) -> Vec<Feed> {
    let Ok(entries) = std::fs::read_dir(folder.join("feeds")) else { return Vec::new() };
    let mut files: Vec<_> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("json")))
        .filter(|p| std::fs::metadata(p).map(|m| m.len() <= MAX_BYTES).unwrap_or(false))
        .collect();
    files.sort();
    files
        .iter()
        .filter_map(|p| {
            let name = p.file_stem()?.to_string_lossy().to_string();
            let raw = std::fs::read_to_string(p).ok()?;
            parse(&name, raw.trim_start_matches('\u{feff}'))
        })
        .take(MAX_FEEDS)
        .collect()
}

/// Write one switch request to `<folder>/feeds/inbox/`. Refused unless the feed exists and
/// offers a switch on that row right now, and the inbox is not already full.
pub fn write_request(folder: &Path, feed: &str, id: &str, on: bool, now_ms: u128) -> Result<(), String> {
    if !safe_name(feed) || !safe_name(id) {
        return Err("That switch name is not allowed.".into());
    }
    let file = folder.join("feeds").join(format!("{feed}.json"));
    let raw = std::fs::read_to_string(&file).map_err(|_| "That feed is not there any more.".to_string())?;
    let parsed = parse(feed, raw.trim_start_matches('\u{feff}')).ok_or("That feed could not be read.")?;
    let row = parsed.rows.iter().find(|r| r.id == id).ok_or("That feed has no such row.")?;
    if row.switch.is_none() {
        return Err("That row has no switch.".into());
    }
    let inbox = folder.join("feeds").join("inbox");
    std::fs::create_dir_all(&inbox).map_err(|e| e.to_string())?;
    let waiting = std::fs::read_dir(&inbox).map(|d| d.count()).unwrap_or(0);
    if waiting >= MAX_INBOX {
        return Err("Too many switch presses are waiting; the feed's writer may be stopped.".into());
    }
    let action = if on { "start" } else { "stop" };
    let body = serde_json::json!({ "feed": feed, "id": id, "action": action, "at": now_ms as u64 });
    let tmp = inbox.join(format!(".{feed}.{id}.{now_ms}.tmp"));
    std::fs::write(&tmp, body.to_string()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, inbox.join(format!("{feed}.{id}.{now_ms}.json"))).map_err(|e| e.to_string())
}

/// HOME's FEEDS card: a switch was pressed.
#[tauri::command]
pub fn feed_switch(store: tauri::State<crate::settings::SettingsStore>, feed: String, id: String, on: bool) -> Result<(), String> {
    let folder = store.get().memory_folder.ok_or("No memory folder is set.")?;
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    write_request(Path::new(&folder), &feed, &id, on, now_ms)
}

/// HOME's FEEDS card.
#[tauri::command]
pub fn memory_feeds(store: tauri::State<crate::settings::SettingsStore>) -> Vec<Feed> {
    store.get().memory_folder.map(|f| read_feeds(Path::new(&f))).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_feed() {
        let f = parse("senses", r#"{"title":"SENSES","updated":"x","rows":[{"label":"Voice","state":"on","detail":"8760"},{"label":"Phone","state":false}]}"#).unwrap();
        assert_eq!(f.title, "SENSES");
        assert_eq!(f.rows.len(), 2);
        assert_eq!(f.rows[0].state, "on");
        assert_eq!(f.rows[1].state, "off");
        assert_eq!(f.rows[1].detail, "");
    }

    #[test]
    fn unknown_state_is_plain_and_title_defaults() {
        let f = parse("x", r#"{"rows":[{"label":"A","state":"<script>"}]}"#).unwrap();
        assert_eq!(f.rows[0].state, "plain");
        assert_eq!(f.title, "X");
    }

    #[test]
    fn not_a_feed() {
        assert!(parse("x", "nope").is_none());
        assert!(parse("x", r#"{"title":"no rows"}"#).is_none());
        assert!(parse("x", "[1,2]").is_none());
    }

    #[test]
    fn caps_rows_and_text() {
        let rows: Vec<String> = (0..40).map(|i| format!(r#"{{"label":"{}","detail":"{}"}}"#, i, "y".repeat(200))).collect();
        let f = parse("x", &format!(r#"{{"rows":[{}]}}"#, rows.join(","))).unwrap();
        assert_eq!(f.rows.len(), MAX_ROWS);
        assert_eq!(f.rows[0].detail.chars().count(), MAX_TEXT);
    }

    #[test]
    fn reads_the_folder_sorted_and_skips_bad_files() {
        let dir = std::env::temp_dir().join(format!("castle-feeds-{}", std::process::id()));
        let feeds = dir.join("feeds");
        std::fs::create_dir_all(&feeds).unwrap();
        std::fs::write(feeds.join("b.json"), r#"{"rows":[{"label":"B"}]}"#).unwrap();
        std::fs::write(feeds.join("a.json"), "\u{feff}{\"rows\":[{\"label\":\"A\"}]}").unwrap();
        std::fs::write(feeds.join("bad.json"), "{").unwrap();
        std::fs::write(feeds.join("note.txt"), r#"{"rows":[]}"#).unwrap();
        let got = read_feeds(&dir);
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(got.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    }

    #[test]
    fn switch_only_with_a_safe_id() {
        let f = parse("s", r#"{"rows":[{"label":"A","id":"voice","switch":"on"},{"label":"B","id":"../x","switch":"on"},{"label":"C","id":"c","switch":"maybe"},{"label":"D","switch":"off"}]}"#).unwrap();
        assert_eq!(f.rows[0].switch.as_deref(), Some("on"));
        assert_eq!(f.rows[1].switch, None);
        assert_eq!(f.rows[1].id, "");
        assert_eq!(f.rows[2].switch, None);
        assert_eq!(f.rows[3].switch, None);
    }

    #[test]
    fn requests_only_for_offered_switches() {
        let dir = std::env::temp_dir().join(format!("castle-feedsw-{}", std::process::id()));
        let feeds = dir.join("feeds");
        std::fs::create_dir_all(&feeds).unwrap();
        std::fs::write(feeds.join("senses.json"), r#"{"rows":[{"label":"Voice","id":"voice","switch":"off"},{"label":"DB","id":"db"}]}"#).unwrap();
        assert!(write_request(&dir, "senses", "voice", true, 1).is_ok());
        assert!(write_request(&dir, "senses", "db", true, 2).is_err());
        assert!(write_request(&dir, "senses", "nope", true, 3).is_err());
        assert!(write_request(&dir, "other", "voice", true, 4).is_err());
        assert!(write_request(&dir, "../senses", "voice", true, 5).is_err());
        let files: Vec<_> = std::fs::read_dir(feeds.join("inbox")).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).collect();
        assert_eq!(files, ["senses.voice.1.json"]);
        let body: Value = serde_json::from_str(&std::fs::read_to_string(feeds.join("inbox").join(&files[0])).unwrap()).unwrap();
        assert_eq!(body["action"], "start");
        for i in 10..40 {
            let _ = write_request(&dir, "senses", "voice", false, i);
        }
        assert_eq!(std::fs::read_dir(feeds.join("inbox")).unwrap().count(), MAX_INBOX);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn no_folder_no_feeds() {
        assert!(read_feeds(Path::new("Z:/does/not/exist")).is_empty());
    }
}
