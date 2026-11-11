//! Copying and moving on a thread, so the window stays alive while gigabytes go: the bytes to do are counted first, the
//! files are copied in chunks (the progress card shows how far, how fast and how long is left, and can stop it), and a
//! name that is already taken in the folder asks what to do (skip, keep both, replace) once or for all the rest. A replaced
//! file goes to the Trash first, so everything a job does can be undone.

use chebrium::app::Waker;
use memrium_de::fsops::{self, Undo};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const CHUNK: usize = 4 << 20;
/// how often the thread wakes the window to show its progress
const TICK: Duration = Duration::from_millis(120);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    Copy,
    Move,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Answer {
    Skip,
    KeepBoth,
    Replace,
}

/// A name already taken: the window asks the user and sends the answer (and whether it is for all the rest).
pub struct Ask {
    pub name: String,
    pub dir: PathBuf,
    /// what is there already is a folder
    pub is_dir: bool,
    reply: Sender<(Answer, bool)>,
}

impl Ask {
    pub fn answer(self, a: Answer, for_all: bool) {
        let _ = self.reply.send((a, for_all));
    }
}

/// What a job did, once it is over.
#[derive(Default, Debug)]
pub struct Outcome {
    pub made: Vec<PathBuf>,
    pub undo: Vec<Undo>,
    pub errors: Vec<String>,
    pub cancelled: bool,
}

#[derive(Default)]
struct Shared {
    total: AtomicU64,
    done: AtomicU64,
    /// the bytes are still being counted
    counting: AtomicBool,
    cancel: AtomicBool,
    current: Mutex<String>,
    ask: Mutex<Option<Ask>>,
    outcome: Mutex<Option<Outcome>>,
}

pub struct Job {
    pub op: Op,
    pub dir: PathBuf,
    pub items: usize,
    pub started: Instant,
    shared: Arc<Shared>,
    /// (when, bytes done) a moment ago, for the speed
    pace: (Instant, u64, f64),
}

/// A view of a running job for the progress card.
pub struct Progress {
    pub done: u64,
    pub total: u64,
    pub counting: bool,
    pub current: String,
    /// bytes a second (smoothed), 0 until known
    pub speed: f64,
}

impl Job {
    /// Start copying or moving `srcs` into `dir`.
    pub fn start(op: Op, srcs: Vec<PathBuf>, dir: PathBuf, waker: Option<Waker>) -> Job {
        let shared = Arc::new(Shared { counting: AtomicBool::new(true), ..Default::default() });
        let me = shared.clone();
        let (d, items) = (dir.clone(), srcs.len());
        std::thread::spawn(move || {
            let ticker = waker.clone().map(|w| {
                let me = me.clone();
                std::thread::spawn(move || {
                    while me.outcome.lock().map(|o| o.is_none()).unwrap_or(false) {
                        std::thread::sleep(TICK);
                        w.wake();
                    }
                })
            });
            let out = run(op, &srcs, &d, &me);
            if let Ok(mut o) = me.outcome.lock() {
                *o = Some(out);
            }
            if let Some(w) = waker {
                w.wake();
            }
            if let Some(t) = ticker {
                let _ = t.join();
            }
        });
        Job { op, dir, items, started: Instant::now(), shared, pace: (Instant::now(), 0, 0.0) }
    }

    pub fn cancel(&self) {
        self.shared.cancel.store(true, Ordering::Relaxed);
        // a question waiting for an answer is answered "skip" so the thread can see the stop
        if let Some(a) = self.shared.ask.lock().ok().and_then(|mut g| g.take()) {
            a.answer(Answer::Skip, true);
        }
    }

    /// The question the job waits on, if any (taken: answer it).
    pub fn take_ask(&self) -> Option<Ask> {
        self.shared.ask.lock().ok().and_then(|mut g| g.take())
    }

    /// The job is over: what it did.
    pub fn take_outcome(&self) -> Option<Outcome> {
        self.shared.outcome.lock().ok().and_then(|mut g| g.take())
    }

    pub fn progress(&mut self) -> Progress {
        let done = self.shared.done.load(Ordering::Relaxed);
        let now = Instant::now();
        let dt = now.duration_since(self.pace.0).as_secs_f64();
        if dt > 0.5 {
            let rate = done.saturating_sub(self.pace.1) as f64 / dt;
            self.pace.2 = if self.pace.2 == 0.0 { rate } else { self.pace.2 * 0.6 + rate * 0.4 };
            self.pace.0 = now;
            self.pace.1 = done;
        }
        Progress { done, total: self.shared.total.load(Ordering::Relaxed), counting: self.shared.counting.load(Ordering::Relaxed), current: self.shared.current.lock().map(|c| c.clone()).unwrap_or_default(), speed: self.pace.2 }
    }
}

fn dev(p: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    fs::symlink_metadata(p).map(|m| m.dev()).ok()
}

/// The bytes of everything under `p` (links are not followed).
fn size_of(p: &Path, cancel: &AtomicBool) -> u64 {
    let Ok(md) = fs::symlink_metadata(p) else { return 0 };
    if md.is_dir() {
        let mut n = 0;
        for e in fs::read_dir(p).into_iter().flatten().flatten() {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            n += size_of(&e.path(), cancel);
        }
        n
    } else if md.is_file() {
        md.len()
    } else {
        0
    }
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "item".into())
}

fn run(op: Op, srcs: &[PathBuf], dir: &Path, sh: &Shared) -> Outcome {
    let mut out = Outcome::default();
    let here = dev(dir);
    // a move within one disk is a rename and costs no bytes
    let renames = |s: &Path| op == Op::Move && here.is_some() && dev(s) == here;
    let total: u64 = srcs.iter().filter(|s| !renames(s)).map(|s| size_of(s, &sh.cancel)).sum();
    sh.total.store(total, Ordering::Relaxed);
    sh.counting.store(false, Ordering::Relaxed);
    let mut for_all: Option<Answer> = None;
    for src in srcs {
        if sh.cancel.load(Ordering::Relaxed) {
            out.cancelled = true;
            break;
        }
        let name = name_of(src);
        if let Ok(mut c) = sh.current.lock() {
            *c = name.clone();
        }
        if src.is_dir() && dir.starts_with(src) {
            out.errors.push(format!("{name}: a folder cannot go into itself"));
            continue;
        }
        if op == Op::Move && src.parent() == Some(dir) {
            continue;
        }
        let mut dst = dir.join(&name);
        // a copy into its own folder is a duplicate; any other taken name asks
        if src.parent() == Some(dir) {
            dst = fsops::unique_path(dir, &name);
        } else if fs::symlink_metadata(&dst).is_ok() {
            let answer = match for_all {
                Some(a) => a,
                None => {
                    let (tx, rx): (Sender<(Answer, bool)>, Receiver<(Answer, bool)>) = channel();
                    if let Ok(mut g) = sh.ask.lock() {
                        *g = Some(Ask { name: name.clone(), dir: dir.to_path_buf(), is_dir: dst.is_dir(), reply: tx });
                    }
                    let (a, all) = rx.recv().unwrap_or((Answer::Skip, true));
                    if all {
                        for_all = Some(a);
                    }
                    a
                }
            };
            if sh.cancel.load(Ordering::Relaxed) {
                out.cancelled = true;
                break;
            }
            match answer {
                Answer::Skip => {
                    if !renames(src) {
                        sh.done.fetch_add(size_of(src, &sh.cancel), Ordering::Relaxed);
                    }
                    continue;
                }
                Answer::KeepBoth => dst = fsops::unique_path(dir, &name),
                Answer::Replace => match fsops::trash(&dst) {
                    Ok(to) => out.undo.push(Undo::Moved { from: dst.clone(), to }),
                    Err(e) => {
                        out.errors.push(format!("{name}: could not put the old one in the Trash: {e}"));
                        continue;
                    }
                },
            }
        }
        let r = if renames(src) {
            fs::rename(src, &dst).map(|_| true)
        } else {
            match copy_tree(src, &dst, sh) {
                Ok(true) if op == Op::Move => fsops::delete_forever(src).map(|_| true),
                other => other,
            }
        };
        match r {
            Ok(true) => {
                out.undo.push(if op == Op::Move { Undo::Moved { from: src.clone(), to: dst.clone() } } else { Undo::Copied(dst.clone()) });
                out.made.push(dst);
            }
            Ok(false) => {
                // stopped half way: what was made of it goes
                let _ = fsops::delete_forever(&dst);
                out.cancelled = true;
                break;
            }
            Err(e) => {
                let _ = if fs::symlink_metadata(&dst).is_ok() && !renames(src) { fsops::delete_forever(&dst) } else { Ok(()) };
                out.errors.push(format!("{name}: {e}"));
            }
        }
    }
    out
}

/// Copy `src` to `dst` (which is free); false when it was stopped.
fn copy_tree(src: &Path, dst: &Path, sh: &Shared) -> std::io::Result<bool> {
    let md = fs::symlink_metadata(src)?;
    if md.is_dir() {
        fs::create_dir(dst)?;
        for e in fs::read_dir(src)? {
            let e = e?;
            if !copy_tree(&e.path(), &dst.join(e.file_name()), sh)? {
                return Ok(false);
            }
        }
        let _ = fs::set_permissions(dst, md.permissions());
        Ok(true)
    } else if md.file_type().is_symlink() {
        std::os::unix::fs::symlink(fs::read_link(src)?, dst)?;
        Ok(true)
    } else if md.is_file() {
        copy_file(src, dst, &md, sh)
    } else {
        // pipes and sockets are not copied
        Ok(true)
    }
}

fn copy_file(src: &Path, dst: &Path, md: &fs::Metadata, sh: &Shared) -> std::io::Result<bool> {
    let mut from = fs::File::open(src)?;
    let mut to = fs::File::create(dst)?;
    let mut buf = vec![0u8; CHUNK.min(md.len().max(4096) as usize)];
    loop {
        if sh.cancel.load(Ordering::Relaxed) {
            return Ok(false);
        }
        let n = from.read(&mut buf)?;
        if n == 0 {
            break;
        }
        to.write_all(&buf[..n])?;
        sh.done.fetch_add(n as u64, Ordering::Relaxed);
    }
    let _ = to.set_permissions(md.permissions());
    if let Ok(t) = md.modified() {
        let _ = to.set_modified(t);
    }
    Ok(true)
}
