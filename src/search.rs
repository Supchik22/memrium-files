//! Searching by name from a folder down: a thread walks the folders breadth first (what is in the folder itself comes
//! first, deeper matches stream in after it), stays on the folder's disk, skips hidden folders unless they are shown,
//! and stops at a few thousand matches. A new query or leaving the folder stops the walk.

use chebrium::app::Waker;
use memrium_de::fsops::Entry;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const MAX_RESULTS: usize = 3000;

#[derive(Default)]
struct Shared {
    found: Mutex<Vec<Entry>>,
    stop: AtomicBool,
    done: AtomicBool,
    /// folders looked into so far
    scanned: std::sync::atomic::AtomicUsize,
}

pub struct Search {
    pub query: String,
    pub root: PathBuf,
    shared: Arc<Shared>,
    /// how many results the window has taken
    pub taken: usize,
}

/// Whether a name matches: every word of the query is in it (case does not matter).
pub fn matches(name: &str, words: &[String]) -> bool {
    let n = name.to_lowercase();
    words.iter().all(|w| n.contains(w.as_str()))
}

pub fn words(query: &str) -> Vec<String> {
    query.split_whitespace().map(str::to_lowercase).collect()
}

impl Search {
    pub fn start(root: PathBuf, query: String, hidden: bool, waker: Option<Waker>) -> Search {
        let shared = Arc::new(Shared::default());
        let me = shared.clone();
        let (r, q) = (root.clone(), query.clone());
        std::thread::spawn(move || {
            walk(&r, &words(&q), hidden, &me, waker.as_ref());
            me.done.store(true, Ordering::Relaxed);
            if let Some(w) = &waker {
                w.wake();
            }
        });
        Search { query, root, shared, taken: 0 }
    }

    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::Relaxed);
    }

    pub fn done(&self) -> bool {
        self.shared.done.load(Ordering::Relaxed)
    }

    pub fn scanned(&self) -> usize {
        self.shared.scanned.load(Ordering::Relaxed)
    }

    /// The results found since the last call.
    pub fn take_new(&mut self) -> Vec<Entry> {
        let Ok(found) = self.shared.found.lock() else { return Vec::new() };
        let new = found[self.taken.min(found.len())..].to_vec();
        self.taken = found.len();
        new
    }
}

impl Drop for Search {
    fn drop(&mut self) {
        self.stop();
    }
}

fn dev(p: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::symlink_metadata(p).map(|m| m.dev()).ok()
}

fn walk(root: &Path, words: &[String], hidden: bool, sh: &Shared, waker: Option<&Waker>) {
    let disk = dev(root);
    let mut queue: VecDeque<PathBuf> = VecDeque::from([root.to_path_buf()]);
    let mut batch: Vec<Entry> = Vec::new();
    let mut last_wake = Instant::now();
    let flush = |batch: &mut Vec<Entry>, last: &mut Instant, force: bool| {
        if batch.is_empty() || (!force && last.elapsed() < Duration::from_millis(150)) {
            return;
        }
        if let Ok(mut f) = sh.found.lock() {
            f.append(batch);
        }
        *last = Instant::now();
        if let Some(w) = waker {
            w.wake();
        }
    };
    let mut total = 0;
    while let Some(dir) = queue.pop_front() {
        if sh.stop.load(Ordering::Relaxed) {
            return;
        }
        sh.scanned.fetch_add(1, Ordering::Relaxed);
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut here: Vec<(String, PathBuf, bool)> = Vec::new();
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') && !hidden {
                continue;
            }
            let ft = e.file_type().ok();
            let is_dir = ft.is_some_and(|t| t.is_dir());
            here.push((name, e.path(), is_dir));
        }
        here.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
        for (name, path, is_dir) in here {
            if matches(&name, words) {
                if let Some(e) = crate::places::entry_of(&path) {
                    batch.push(e);
                    total += 1;
                    if total >= MAX_RESULTS {
                        flush(&mut batch, &mut last_wake, true);
                        return;
                    }
                }
            }
            // (links to folders are not followed: a loop or a walk over the whole disk)
            if is_dir && dev(&path) == disk {
                queue.push_back(path);
            }
        }
        flush(&mut batch, &mut last_wake, false);
    }
    flush(&mut batch, &mut last_wake, true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_finds_from_the_folder_down_nearest_first() {
        let dir = std::env::temp_dir().join(format!("memrium-search-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a/b")).unwrap();
        std::fs::create_dir_all(dir.join(".hid")).unwrap();
        for f in ["Report one.txt", "a/b/report two.md", "a/other.txt", ".hid/report.txt"] {
            std::fs::write(dir.join(f), "x").unwrap();
        }
        let mut s = Search::start(dir.clone(), "REPORT".into(), false, None);
        let t = Instant::now();
        while !s.done() && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(5));
        }
        let names: Vec<String> = s.take_new().into_iter().map(|e| e.name).collect();
        assert_eq!(names, vec!["Report one.txt", "report two.md"]);
        assert!(matches("My Holiday Photo.jpg", &words("photo holiday")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
