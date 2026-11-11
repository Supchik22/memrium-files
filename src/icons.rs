//! The pictures of the entries: the icon theme's folder and file-type icons (as Dolphin shows them), the icons that
//! launchers declare, thumbnails of pictures and folder icons with a few of their photos on them. The indexes of the icon
//! themes are read on threads at start; thumbnails are made by a few worker threads, only for what is on screen.

use chebrium::app::Waker;
use chebrium::picture::Pic;
use memrium_de::apps::{IconIndex, IconSlot};
use memrium_de::fsops::{self, Entry, Kind};
use memrium_de::thumbs;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};

/// the icons are drawn from pictures this big (sharp up to the largest icon size on a 1.5x screen)
const PX: u32 = 128;
/// file-type icons made per frame (each is an SVG drawn on the processor): the rest come in the next frames
const PER_FRAME: usize = 8;
/// thumbnails kept; past this the oldest go (a big photo folder is not kept whole in memory)
const MAX_THUMBS: usize = 400;

/// A thumbnail to make: its key, the file, and for a folder the folder icon the photos go onto.
type Job = (String, PathBuf, Option<image::RgbaImage>);

#[derive(Default)]
struct Jobs {
    queue: Vec<Job>,
    taken: HashSet<String>,
}

/// A picture made for the window, and the straight RGBA it was made from (the picture under the pointer in a drag).
#[derive(Clone)]
pub struct Icon {
    pub pic: Arc<Pic>,
    pub rgba: Arc<image::RgbaImage>,
}

impl Icon {
    fn new(img: image::RgbaImage) -> Icon {
        Icon { pic: Arc::new(Pic::from_rgba(img.width(), img.height(), img.as_raw())), rgba: Arc::new(img) }
    }
}

pub struct Icons {
    mime: Arc<IconSlot>,
    apps: Arc<IconSlot>,
    made: HashMap<String, Option<Icon>>,
    thumbs: HashMap<String, Option<Icon>>,
    /// the order thumbnails came in, oldest first
    thumb_order: std::collections::VecDeque<String>,
    jobs: Arc<(Mutex<Jobs>, Condvar)>,
    tx: Sender<(String, Option<image::RgbaImage>)>,
    rx: Receiver<(String, Option<image::RgbaImage>)>,
    /// file-type icons still allowed this frame
    budget: usize,
    /// something asked for could not be had yet: draw again soon
    pub waiting: bool,
    /// thumbnails wanted this frame (handed to the workers by `end_frame`)
    wanted: Vec<Job>,
    workers: bool,
    waker: Option<Waker>,
}

fn thumb_key(e: &Entry) -> String {
    format!("th:{}:{}", e.path.display(), e.modified)
}

fn folder_key(e: &Entry) -> String {
    format!("fp:{}:{}", e.path.display(), e.modified)
}

impl Icons {
    pub fn new() -> Icons {
        let (tx, rx) = channel();
        Icons {
            mime: Default::default(),
            apps: Default::default(),
            made: HashMap::new(),
            thumbs: HashMap::new(),
            thumb_order: Default::default(),
            jobs: Default::default(),
            tx,
            rx,
            budget: PER_FRAME,
            waiting: false,
            wanted: Vec::new(),
            workers: false,
            waker: None,
        }
    }

    /// Read the icon themes (on threads; the window is woken when they are ready) and start the thumbnail workers.
    pub fn start(&mut self, waker: Waker) {
        for (slot, mime) in [(self.mime.clone(), true), (self.apps.clone(), false)] {
            let w = waker.clone();
            std::thread::spawn(move || {
                slot.set(if mime { IconIndex::build_mime() } else { IconIndex::build() });
                w.wake();
            });
        }
        self.waker = Some(waker);
    }

    fn start_workers(&mut self) {
        if self.workers {
            return;
        }
        self.workers = true;
        let n = std::thread::available_parallelism().map_or(2, |n| n.get().clamp(2, 3));
        for _ in 0..n {
            let jobs = self.jobs.clone();
            let tx = self.tx.clone();
            let waker = self.waker.clone();
            std::thread::spawn(move || loop {
                let (key, path, base) = {
                    let (lock, cv) = &*jobs;
                    let mut j = lock.lock().unwrap();
                    while j.queue.is_empty() {
                        j = cv.wait(j).unwrap();
                    }
                    let job = j.queue.remove(0);
                    j.taken.insert(job.0.clone());
                    job
                };
                let img = match base {
                    Some(base) => thumbs::folder_preview(&base, &path),
                    None => thumbs::thumbnail(&path),
                };
                if tx.send((key, img)).is_err() {
                    break;
                }
                if let Some(w) = &waker {
                    w.wake();
                }
            });
        }
    }

    /// Call before drawing a frame: takes the thumbnails that are done.
    pub fn begin_frame(&mut self) {
        self.budget = PER_FRAME;
        self.waiting = false;
        self.wanted.clear();
        while let Ok((key, img)) = self.rx.try_recv() {
            self.jobs.0.lock().unwrap().taken.remove(&key);
            self.thumbs.insert(key.clone(), img.map(Icon::new));
            self.thumb_order.push_back(key);
        }
        while self.thumb_order.len() > MAX_THUMBS {
            if let Some(k) = self.thumb_order.pop_front() {
                self.thumbs.remove(&k);
            }
        }
    }

    /// Call after the frame: only what was on screen is made (a fast scroll leaves nothing behind to decode).
    pub fn end_frame(&mut self) {
        let wanted = std::mem::take(&mut self.wanted);
        if wanted.is_empty() {
            return;
        }
        self.start_workers();
        let (lock, cv) = &*self.jobs;
        let mut j = lock.lock().unwrap();
        j.queue = wanted.into_iter().filter(|(k, ..)| !j.taken.contains(k)).collect();
        if !j.queue.is_empty() {
            cv.notify_all();
        }
    }

    /// A named icon from a theme index, made once.
    fn named(&mut self, key: String, names: &[&str], apps: bool) -> Option<Icon> {
        if let Some(i) = self.made.get(&key) {
            return i.clone();
        }
        let slot = if apps { &self.apps } else { &self.mime };
        let Some(index) = slot.get() else {
            self.waiting = true;
            return None;
        };
        if self.budget == 0 {
            self.waiting = true;
            return None;
        }
        self.budget -= 1;
        let icon = names.iter().find_map(|n| index.load(n, PX)).map(Icon::new);
        self.made.insert(key, icon.clone());
        icon
    }

    /// A program's icon by the name its desktop file gives (Open With).
    pub fn app(&mut self, name: &str) -> Option<Icon> {
        if name.is_empty() {
            return None;
        }
        // an icon given as a file
        if name.starts_with('/') {
            let key = format!("appf:{name}");
            if let Some(i) = self.made.get(&key) {
                return i.clone();
            }
            let i = memrium_de::apps::load_icon_file(std::path::Path::new(name), PX).map(Icon::new);
            self.made.insert(key, i.clone());
            return i;
        }
        self.named(format!("app:{name}"), &[name], true)
    }

    /// The theme's icon for the kind of `e` (or the icon its launcher declares).
    pub fn kind(&mut self, e: &Entry) -> Option<Icon> {
        if let Some((_, icon)) = e.app.as_ref().filter(|(_, i)| !i.is_empty()) {
            let icon = icon.clone();
            if let Some(i) = self.named(format!("app:{icon}"), &[icon.as_str()], true) {
                return Some(i);
            }
        }
        let names = fsops::icon_names(e);
        self.named(format!("fi:{}", names[0]), &names, false)
    }

    /// What to draw for `e`: with `thumb`, the picture's own thumbnail (or the folder with its photos) once it is made,
    /// the file-type icon until then. None: no icon theme has one (draw the glyph).
    pub fn get(&mut self, e: &Entry, thumb: bool) -> Option<Icon> {
        let base = self.kind(e);
        if thumb && (e.is_dir || e.kind == Kind::Image) && !fsops::is_computer(&e.path) {
            let key = if e.is_dir { folder_key(e) } else { thumb_key(e) };
            match self.thumbs.get(&key) {
                Some(Some(t)) => return Some(t.clone()),
                Some(None) => {}
                None => {
                    let folder_base = if e.is_dir { base.as_ref().map(|b| (*b.rgba).clone()) } else { None };
                    // a folder waits for its icon before its photos can go onto it
                    if !e.is_dir || folder_base.is_some() {
                        self.wanted.push((key, e.path.clone(), folder_base));
                    }
                }
            }
        }
        base
    }

    /// The icon theme changed, or the folder was read again: make the icons anew.
    pub fn forget(&mut self) {
        self.made.clear();
    }
}
