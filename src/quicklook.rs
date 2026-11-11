//! Looking into a file without opening it: Space shows a big preview over the window (pictures, a frame of a film or the
//! cover of a song, the start of a text, what is in a folder), the arrows go on to the next file while it is open. The
//! same previews fill the Information panel of the Dolphin look. They are made on threads and a few are kept.

use chebrium::app::Waker;
use chebrium::draw::Align;
use chebrium::picture::Pic;
use chebrium::ui::{inside, Ui};
use chebrium::widgets::{self, fit, round_button, Round};
use memrium_de::fsops::{self, Entry, Kind};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// the longest side of a picture preview
const MAX_SIDE: u32 = 1600;
const TEXT_BYTES: usize = 48 * 1024;
const TEXT_LINES: usize = 300;
const KEEP: usize = 12;

#[derive(Clone, Default)]
pub struct Preview {
    pub pic: Option<Arc<Pic>>,
    pub text: Option<Vec<String>>,
    /// name/value lines: dimensions, length, what a folder holds
    pub facts: Vec<(String, String)>,
}

#[derive(Default)]
pub struct Previews {
    made: Arc<Mutex<HashMap<PathBuf, Preview>>>,
    asked: Vec<PathBuf>,
    waker: Option<Waker>,
}

impl Previews {
    pub fn start(&mut self, waker: Waker) {
        self.waker = Some(waker);
    }

    /// The preview of `e` when it is made; asks for it otherwise.
    pub fn get(&mut self, e: &Entry) -> Option<Preview> {
        if let Some(p) = self.made.lock().ok().and_then(|m| m.get(&e.path).cloned()) {
            return Some(p);
        }
        if !self.asked.contains(&e.path) {
            self.asked.push(e.path.clone());
            let (made, waker, e) = (self.made.clone(), self.waker.clone(), e.clone());
            std::thread::spawn(move || {
                let p = make(&e);
                if let Ok(mut m) = made.lock() {
                    if m.len() >= KEEP {
                        // the oldest go (any will do: they are cheap to make again)
                        if let Some(k) = m.keys().next().cloned() {
                            m.remove(&k);
                        }
                    }
                    m.insert(e.path.clone(), p);
                }
                if let Some(w) = waker {
                    w.wake();
                }
            });
        }
        None
    }

    /// A file changed (rotated, renamed): its preview is made again.
    pub fn forget(&mut self) {
        self.asked.clear();
        if let Ok(mut m) = self.made.lock() {
            m.clear();
        }
    }
}

fn pic_of(img: image::DynamicImage) -> Arc<Pic> {
    let img = if img.width().max(img.height()) > MAX_SIDE { img.thumbnail(MAX_SIDE, MAX_SIDE) } else { img };
    let rgba = img.to_rgba8();
    Arc::new(Pic::from_rgba(rgba.width(), rgba.height(), rgba.as_raw()))
}

/// One frame of a film (a second in) or the cover of a song, by ffmpeg.
fn frame_of(path: &Path, cover: bool) -> Option<image::DynamicImage> {
    let mut c = std::process::Command::new("ffmpeg");
    c.args(["-v", "quiet", "-nostdin"]);
    if !cover {
        c.args(["-ss", "1"]);
    }
    c.arg("-i").arg(path);
    if cover {
        c.arg("-an");
    }
    c.args(["-frames:v", "1", "-vf", "scale='min(1280,iw)':-2", "-f", "image2pipe", "-vcodec", "png", "-"]);
    let out = c.stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
    image::load_from_memory(&out.stdout).ok()
}

/// "1:23:45" / "3:07" of a film or song, by ffprobe.
fn duration_of(path: &Path) -> Option<String> {
    let out = std::process::Command::new("ffprobe").args(["-v", "quiet", "-show_entries", "format=duration", "-of", "csv=p=0"]).arg(path).stdin(std::process::Stdio::null()).output().ok()?;
    let secs: f64 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    let s = secs.round() as u64;
    Some(if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) })
}

fn is_text(e: &Entry) -> bool {
    matches!(e.kind, Kind::Code | Kind::Text) && !e.name.to_lowercase().ends_with(".pdf") && !e.name.to_lowercase().ends_with(".docx") && !e.name.to_lowercase().ends_with(".odt") && !e.name.to_lowercase().ends_with(".doc") && !e.name.to_lowercase().ends_with(".epub") || (e.kind == Kind::Other && memrium_de::mimeapps::mime_of(&e.path).starts_with("text/"))
}

pub fn make(e: &Entry) -> Preview {
    let mut p = Preview::default();
    if e.is_dir {
        let (mut dirs, mut files, mut bytes) = (0usize, 0usize, 0u64);
        for x in std::fs::read_dir(&e.path).into_iter().flatten().flatten().take(20_000) {
            match x.metadata() {
                Ok(m) if m.is_dir() => dirs += 1,
                Ok(m) => {
                    files += 1;
                    bytes += m.len();
                }
                Err(_) => {}
            }
        }
        p.facts.push(("Contains".into(), chebrium::i18n::trf("{} folders, {} files", &[dirs.to_string(), files.to_string()])));
        if bytes > 0 {
            p.facts.push(("Files here".into(), fsops::human_size(bytes)));
        }
        return p;
    }
    p.facts.push(("Size".into(), fsops::human_size(e.size)));
    match e.kind {
        Kind::Image => {
            if let Ok((w, h)) = image::image_dimensions(&e.path) {
                p.facts.push(("Dimensions".into(), format!("{w} × {h}")));
            }
            let img = if e.name.to_lowercase().ends_with(".svg") { memrium_de::apps::load_icon_file(&e.path, 1024).map(image::DynamicImage::ImageRgba8) } else { image::open(&e.path).ok() };
            p.pic = img.map(pic_of);
        }
        Kind::Video | Kind::Audio => {
            if let Some(d) = duration_of(&e.path) {
                p.facts.push(("Length".into(), d));
            }
            p.pic = frame_of(&e.path, e.kind == Kind::Audio).map(|i| {
                if e.kind == Kind::Video {
                    p.facts.push(("Frame".into(), format!("{} × {}", i.width(), i.height())));
                }
                pic_of(i)
            });
        }
        _ if is_text(e) => {
            let mut buf = vec![0u8; TEXT_BYTES];
            let n = std::fs::File::open(&e.path).and_then(|mut f| std::io::Read::read(&mut f, &mut buf)).unwrap_or(0);
            let text = String::from_utf8_lossy(&buf[..n]);
            let mut lines: Vec<String> = text.lines().take(TEXT_LINES).map(|l| l.replace('\t', "    ")).collect();
            if n == TEXT_BYTES || text.lines().count() > TEXT_LINES {
                lines.push("…".into());
            }
            p.facts.push(("Lines".into(), if n < TEXT_BYTES { text.lines().count().to_string() } else { format!("{}+", text.lines().count()) }));
            p.text = Some(lines);
        }
        _ => {}
    }
    let mime = memrium_de::mimeapps::mime_of(&e.path);
    p.facts.push(("Type".into(), memrium_de::mimeapps::describe(&mime)));
    p
}

/// What the Quick Look card wants done after a frame.
pub enum Action {
    None,
    Close,
    Open,
}

/// Draw the Quick Look card of `e` over the window. `icon` is drawn while the preview is being made and for files that
/// have none.
pub fn draw(ui: &mut Ui<'_>, size: [f32; 2], e: &Entry, preview: Option<&Preview>, icon: Option<&Arc<Pic>>, scroll: &mut f32) -> Action {
    let [w, h] = size;
    let (ink, dark) = (ui.ink, ui.dark);
    let t = ui.ease(77_001, 1.0, 16.0);
    ui.fill([0.0, 0.0, w, h], 0.0, [0.0; 3], 0.30 * t);
    let cw = (w - 80.0).clamp(240.0, 1100.0);
    let ch = (h - 70.0).clamp(200.0, 860.0);
    let s = 0.94 + 0.06 * t;
    let card = [(w - cw * s) / 2.0, (h - ch * s) / 2.0, cw * s, ch * s];
    let paper = if dark { [0.11, 0.115, 0.13] } else { [0.985, 0.985, 0.99] };
    ui.fill([card[0] - 1.0, card[1] - 1.0, card[2] + 2.0, card[3] + 2.0], 15.0, [0.0; 3], 0.18 * t);
    ui.fill(card, 14.0, paper, 0.97 * t);
    // the header: the name, Open, close
    let head = 46.0;
    let name = fit(ui, &e.name, 14.0, true, card[2] - 220.0);
    ui.text_raw(card[0] + 18.0, card[1] + 28.0, &name, 14.0, true, ink, 0.95 * t, Align::Left);
    let mut act = Action::None;
    if round_button(ui, card[0] + card[2] - 26.0, card[1] + head / 2.0, "x", Round::ink(28.0).glyph(15.0)) {
        act = Action::Close;
    }
    let ob = [card[0] + card[2] - 52.0 - 84.0, card[1] + 9.0, 84.0, 28.0];
    if ui.button(ob, if e.is_dir { "Open" } else { "Open" }, true) {
        act = Action::Open;
    }
    ui.fill([card[0], card[1] + head, card[2], 1.0], 0.0, ink, 0.08 * t);
    let body = [card[0] + 16.0, card[1] + head + 12.0, card[2] - 32.0, card[3] - head - 12.0 - 44.0];
    match preview {
        Some(Preview { pic: Some(pic), .. }) => {
            let (pw, ph) = (pic.w as f32, pic.h as f32);
            // fitted, never blown up past twice its size
            let k = (body[2] / pw).min(body[3] / ph).min(2.0);
            let (dw, dh) = (pw * k, ph * k);
            let r = [body[0] + (body[2] - dw) / 2.0, body[1] + (body[3] - dh) / 2.0, dw, dh];
            ui.picture(r, pic, [0.0, 0.0, pw, ph], t);
        }
        Some(Preview { text: Some(lines), .. }) => {
            let lh = 17.0;
            let area = widgets::scroll_area(ui, body, lines.len() as f32 * lh + 8.0, scroll);
            for i in area.rows(lh, lines.len()) {
                let y = area.y(4.0 + i as f32 * lh);
                let line = fit(ui, &lines[i], 12.5, false, body[2] - 60.0);
                ui.text_raw(body[0] + 40.0, y + 13.0, &line, 12.5, false, ink, 0.88 * t, Align::Left);
                ui.text_raw(body[0] + 30.0, y + 13.0, &(i + 1).to_string(), 10.5, false, ink, 0.3 * t, Align::Right);
            }
            area.end(ui);
        }
        _ => {
            let (cx, cy) = (body[0] + body[2] / 2.0, body[1] + body[3] / 2.0 - 20.0);
            match icon {
                Some(pic) => ui.picture([cx - 64.0, cy - 64.0, 128.0, 128.0], pic, [0.0, 0.0, pic.w as f32, pic.h as f32], t),
                None => ui.glyph(cx, cy, 72.0, e.kind.icon(), e.kind.tint(ui.accent), t),
            }
            if preview.is_none() {
                ui.text(cx, cy + 96.0, "Loading…", 12.5, false, ink, 0.5 * t, Align::Center);
            }
        }
    }
    // the facts along the foot
    if let Some(p) = preview {
        let line: Vec<String> = p.facts.iter().map(|(k, v)| format!("{}: {v}", chebrium::i18n::tr(k))).collect();
        let line = fit(ui, &line.join("   ·   "), 12.0, false, card[2] - 36.0);
        ui.text_raw(card[0] + card[2] / 2.0, card[1] + card[3] - 17.0, &line, 12.0, false, ink, 0.6 * t, Align::Center);
    }
    if ui.click && !inside(ui.mouse, card) {
        act = Action::Close;
    }
    act
}
