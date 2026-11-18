//! A local helper asks Castle to press Esc once in the main session (Alice Pocket's Stop button,
//! alice-fixes), through a folder, the same way `voice_inbox` lets a helper type.
//!
//! `<memory folder>/inbox/control/stop-<anything>.txt` whose whole text is `stop`. Castle trusts
//! nothing in the folder: only plain `stop-` names, only that one word, and a request older than
//! fifteen seconds is dropped unused, so a Stop sent long ago never interrupts a later task.
//! Nothing but Esc can be pressed through this folder, and never twice in a row: two quick Escs
//! open Claude Code's rewind menu, so the presses keep a gap of at least two seconds.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};
use tauri::{AppHandle, Manager};

const STALE_AFTER: Duration = Duration::from_secs(15);
const MIN_GAP: Duration = Duration::from_secs(2);
const MAX_FILES: usize = 20;
const MAX_BYTES: u64 = 64;
const POLL: Duration = Duration::from_millis(400);
const ESC: &[u8] = b"\x1b";

/// What to do with one file in the folder.
#[derive(Debug, PartialEq)]
pub enum Item {
    /// a fresh, well-formed Stop: press Esc (at most once a round), then delete the file
    Stop(PathBuf),
    /// delete the file without pressing anything (too old, too big, not the word)
    Drop(PathBuf),
}

fn plain_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".txt") else { return false };
    stem.starts_with("stop-")
        && stem.len() <= 64
        && stem.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// The folder, oldest name first. Files with other names are left alone (not ours to delete).
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
                Ok(text) if text.trim() == "stop" => Item::Stop(path),
                _ => Item::Drop(path),
            }
        })
        .collect()
}

/// Whether this round presses Esc: only for a Stop, and never within MIN_GAP of the last press.
pub fn should_press(stops: usize, since_last: Option<Duration>) -> bool {
    stops > 0 && since_last.is_none_or(|gap| gap >= MIN_GAP)
}

pub fn inbox_dir(memory_folder: &Path) -> PathBuf {
    memory_folder.join("inbox").join("control")
}

/// Watches the folder for the whole life of Castle.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let mut last: Option<Instant> = None;
        loop {
            std::thread::sleep(POLL);
            let Some(folder) = app.state::<crate::settings::SettingsStore>().get().memory_folder else {
                continue;
            };
            let items = take(&inbox_dir(Path::new(&folder)), SystemTime::now());
            let stops = items.iter().filter(|i| matches!(i, Item::Stop(_))).count();
            if should_press(stops, last.map(|t| t.elapsed())) {
                let host = &app.state::<crate::session::MainHost>().0;
                // no session: nothing is running, so there is nothing to stop
                if host.started().is_some() && host.write(ESC).is_ok() {
                    last = Some(Instant::now());
                }
            }
            for item in items {
                let (Item::Stop(path) | Item::Drop(path)) = item;
                let _ = std::fs::remove_file(path);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("castle-control-inbox-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn only_the_word_stop_in_a_stop_file_presses() {
        let d = dir("word");
        fs::write(d.join("stop-001.txt"), "stop\n").unwrap();
        fs::write(d.join("stop-002.txt"), "stop now please").unwrap();
        fs::write(d.join("stop-003.txt"), "\u{1b}[200~rm -rf").unwrap();
        let items = take(&d, SystemTime::now());
        assert_eq!(items, vec![
            Item::Stop(d.join("stop-001.txt")),
            Item::Drop(d.join("stop-002.txt")),
            Item::Drop(d.join("stop-003.txt")),
        ]);
    }

    #[test]
    fn other_names_are_left_alone() {
        let d = dir("names");
        fs::write(d.join("phone-1.txt"), "stop").unwrap();
        fs::write(d.join("STOP-1.txt"), "stop").unwrap();
        fs::write(d.join("stop-1.txt.part"), "stop").unwrap();
        fs::write(d.join("stop 1.txt"), "stop").unwrap();
        assert!(take(&d, SystemTime::now()).is_empty());
    }

    #[test]
    fn an_old_or_big_request_is_dropped_unused() {
        let d = dir("old");
        fs::write(d.join("stop-1.txt"), "stop").unwrap();
        let later = SystemTime::now() + STALE_AFTER + Duration::from_secs(1);
        assert_eq!(take(&d, later), vec![Item::Drop(d.join("stop-1.txt"))]);
        fs::write(d.join("stop-2.txt"), format!("stop{}", " ".repeat(100))).unwrap();
        assert_eq!(take(&d, SystemTime::now())[1], Item::Drop(d.join("stop-2.txt")));
    }

    #[test]
    fn presses_once_and_keeps_a_gap() {
        assert!(should_press(1, None));
        assert!(should_press(3, None), "several Stops in one round are one Esc");
        assert!(!should_press(0, None));
        assert!(!should_press(1, Some(Duration::from_millis(500))), "two quick Escs open the rewind menu");
        assert!(should_press(1, Some(MIN_GAP)));
    }
}
