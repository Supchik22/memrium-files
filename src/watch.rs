//! Watching what the window shows: the open folder (files made, removed or renamed by other programs), the preferences
//! file (the shell switched the look with a theme template) and the mounted disks. A thread looks every so often and
//! wakes the window only when something changed, so an idle window costs nothing but a few `stat` calls.

use chebrium::app::Waker;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

const EVERY: Duration = Duration::from_millis(1500);

#[derive(Default)]
struct Shared {
    /// the folder shown (None: nothing to watch, the Computer view)
    dir: Mutex<Option<PathBuf>>,
    dir_changed: AtomicBool,
    prefs_changed: AtomicBool,
    mounts_changed: AtomicBool,
}

#[derive(Clone, Default)]
pub struct Watch(Arc<Shared>);

/// What changed since the last look.
#[derive(Default, Debug)]
pub struct Changes {
    pub dir: bool,
    pub prefs: bool,
    pub mounts: bool,
}

fn stamp(p: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

impl Watch {
    pub fn start(&self, waker: Waker) {
        let me = self.0.clone();
        std::thread::spawn(move || {
            let prefs = memrium_de::files::FilesPrefs::path();
            let (mut dir_seen, mut prefs_seen) = (None::<(PathBuf, Option<SystemTime>)>, stamp(&prefs));
            let mut mounts_seen = std::fs::read_to_string("/proc/self/mounts").unwrap_or_default();
            loop {
                std::thread::sleep(EVERY);
                let mut wake = false;
                let dir = me.dir.lock().ok().and_then(|d| d.clone());
                if let Some(dir) = dir {
                    let now = stamp(&dir);
                    match &dir_seen {
                        Some((d, s)) if *d == dir && *s != now => {
                            me.dir_changed.store(true, Ordering::Relaxed);
                            wake = true;
                        }
                        _ => {}
                    }
                    dir_seen = Some((dir, now));
                } else {
                    dir_seen = None;
                }
                let now = stamp(&prefs);
                if now != prefs_seen {
                    prefs_seen = now;
                    me.prefs_changed.store(true, Ordering::Relaxed);
                    wake = true;
                }
                let mounts = std::fs::read_to_string("/proc/self/mounts").unwrap_or_default();
                if mounts != mounts_seen {
                    mounts_seen = mounts;
                    me.mounts_changed.store(true, Ordering::Relaxed);
                    wake = true;
                }
                if wake {
                    waker.wake();
                }
            }
        });
    }

    /// The folder to watch from now on (its own changes up to here are not reported).
    pub fn follow(&self, dir: Option<PathBuf>) {
        if let Ok(mut d) = self.0.dir.lock() {
            *d = dir;
        }
        self.0.dir_changed.store(false, Ordering::Relaxed);
    }

    pub fn take(&self) -> Changes {
        Changes { dir: self.0.dir_changed.swap(false, Ordering::Relaxed), prefs: self.0.prefs_changed.swap(false, Ordering::Relaxed), mounts: self.0.mounts_changed.swap(false, Ordering::Relaxed) }
    }
}
