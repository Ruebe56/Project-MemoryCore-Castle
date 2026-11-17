//! Voice (or any local helper) talks to the companion through a folder.
//!
//! A speech listener writes what it heard to `<memory folder>/inbox/voice/<name>.txt`; Castle
//! picks each file up and sends its text exactly as Chat's own send button does (`send_message`:
//! into the main session, or queued while the session is busy), then deletes the file. No port,
//! no window raising, no keystrokes into whatever happens to have focus.
//!
//! Castle trusts nothing in the folder: only plain names (`a-z0-9_-`, `.txt`), at most 8 KB,
//! at most 20 a round, and a file older than two minutes is dropped unsent, so something said
//! long ago is never typed into a later conversation.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Manager};

const MAX_BYTES: u64 = 8 * 1024;
const MAX_FILES: usize = 20;
const STALE_AFTER: Duration = Duration::from_secs(120);
const POLL: Duration = Duration::from_millis(400);

/// What to do with one file in the inbox.
#[derive(Debug, PartialEq)]
pub enum Item {
    /// send this text, then delete the file
    Send(PathBuf, String),
    /// delete the file without sending (too old, too big, empty, not text)
    Drop(PathBuf),
}

fn plain_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".txt") else { return false };
    !stem.is_empty()
        && stem.len() <= 64
        && stem.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// The inbox, oldest name first. Files with other names are left alone (not ours to delete).
pub fn take(dir: &Path, now: SystemTime) -> Vec<Item> {
    let Ok(read) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<String> = read
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| plain_name(n))
        .collect();
    names.sort();
    names.truncate(MAX_FILES);
    names
        .into_iter()
        .map(|name| {
            let path = dir.join(&name);
            let Ok(meta) = std::fs::metadata(&path) else { return Item::Drop(path) };
            let old = meta
                .modified()
                .ok()
                .and_then(|m| now.duration_since(m).ok())
                .is_some_and(|age| age > STALE_AFTER);
            if old || meta.len() > MAX_BYTES {
                return Item::Drop(path);
            }
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    // one line of speech: no control characters reach the terminal
                    let clean: String = text
                        .chars()
                        .map(|c| if c.is_control() { ' ' } else { c })
                        .collect::<String>()
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ");
                    if clean.is_empty() { Item::Drop(path) } else { Item::Send(path, clean) }
                }
                Err(_) => Item::Drop(path),
            }
        })
        .collect()
}

pub fn inbox_dir(memory_folder: &Path) -> PathBuf {
    memory_folder.join("inbox").join("voice")
}

/// Watches the inbox for the whole life of Castle.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(POLL);
        let Some(folder) = app.state::<crate::settings::SettingsStore>().get().memory_folder else {
            continue;
        };
        for item in take(&inbox_dir(Path::new(&folder)), SystemTime::now()) {
            match item {
                Item::Drop(path) => {
                    let _ = std::fs::remove_file(path);
                }
                Item::Send(path, text) => {
                    // no session yet: leave the file; it is retried until it goes stale
                    if crate::messages::send_message(text, None, app.clone()).is_ok() {
                        let _ = std::fs::remove_file(path);
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("castle-voice-inbox-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn sends_plain_text_in_name_order_on_one_line() {
        let d = dir("order");
        fs::write(d.join("002.txt"), "second").unwrap();
        fs::write(d.join("001.txt"), "  hello\r\nthere\tAlice \u{1b}[31m ").unwrap();
        let got = take(&d, SystemTime::now());
        assert_eq!(
            got,
            vec![
                Item::Send(d.join("001.txt"), "hello there Alice [31m".into()),
                Item::Send(d.join("002.txt"), "second".into()),
            ]
        );
    }

    #[test]
    fn drops_old_big_and_empty_files_and_ignores_strange_names() {
        let d = dir("drop");
        fs::write(d.join("empty.txt"), " \n ").unwrap();
        fs::write(d.join("big.txt"), "x".repeat(9 * 1024)).unwrap();
        fs::write(d.join("old.txt"), "said long ago").unwrap();
        fs::write(d.join("Not-Ours.TXT"), "leave me").unwrap();
        fs::write(d.join("..evil.txt"), "no").unwrap();
        fs::write(d.join("note.json"), "{}").unwrap();
        // "old" is judged from a clock three minutes ahead
        let later = SystemTime::now() + Duration::from_secs(180);
        let got = take(&d, later);
        assert!(got.iter().all(|i| matches!(i, Item::Drop(_))), "{got:?}");
        assert_eq!(got.len(), 3, "only empty/big/old are ours: {got:?}");
        let now = take(&d, SystemTime::now());
        assert!(now.contains(&Item::Send(d.join("old.txt"), "said long ago".into())));
        assert!(now.contains(&Item::Drop(d.join("big.txt"))));
    }

    #[test]
    fn takes_at_most_twenty_a_round() {
        let d = dir("cap");
        for i in 0..25 {
            fs::write(d.join(format!("{i:03}.txt")), "hi").unwrap();
        }
        assert_eq!(take(&d, SystemTime::now()).len(), 20);
    }

    #[test]
    fn a_missing_inbox_is_simply_empty() {
        assert!(take(Path::new("Z:/no/such/folder/inbox/voice"), SystemTime::now()).is_empty());
    }
}
