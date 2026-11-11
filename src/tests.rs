use super::*;
use chebrium::canvas::Canvas;
use chebrium::soft::{Gfx, SoftDraw};
use chebrium::style::Style;
use chebrium::ui::State;
use std::time::Duration;

/// The tests keep their settings, bookmarks, Trash and recent files in a folder of their own (never the user's).
fn sandbox() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let d = std::env::temp_dir().join(format!("memrium-files-env-{}", std::process::id()));
        let _ = std::fs::create_dir_all(d.join("config"));
        let _ = std::fs::create_dir_all(d.join("data"));
        std::env::set_var("XDG_CONFIG_HOME", d.join("config"));
        std::env::set_var("XDG_DATA_HOME", d.join("data"));
        std::env::remove_var("WAYLAND_DISPLAY");
    });
}

fn setup(tag: &str) -> (PathBuf, FilesApp) {
    sandbox();
    let dir = std::env::temp_dir().join(format!("memrium-files-app-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("note.txt"), "x").unwrap();
    std::fs::write(dir.join("Alpha.md"), "x").unwrap();
    let dir = dir.canonicalize().unwrap();
    (dir.clone(), FilesApp::new(dir, None))
}

/// Wait for the jobs to finish (answering a question with `answer` for all).
fn finish_jobs(app: &mut FilesApp, answer: Option<jobs::Answer>) {
    let t = Instant::now();
    while !app.jobs.is_empty() && t.elapsed() < Duration::from_secs(10) {
        app.poll_jobs();
        if let (Some(a), Some(ans)) = (app.ask.take(), answer) {
            a.answer(ans, true);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(app.jobs.is_empty(), "jobs did not finish");
}

#[test]
fn folders_come_first_and_the_filter_narrows_the_list() {
    let (dir, mut app) = setup("filter");
    assert_eq!(app.shown_len(), 3);
    assert!(app.at(0).is_dir);
    app.filter = "alp".into();
    app.apply_filter();
    assert_eq!(app.shown_len(), 1);
    assert_eq!(app.at(0).path, dir.join("Alpha.md"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn going_into_a_folder_and_back_up_selects_where_we_came_from() {
    let (dir, mut app) = setup("nav");
    app.go(dir.join("sub"));
    assert_eq!(app.cwd, dir.join("sub"));
    assert!(app.can_back());
    app.go_up();
    assert_eq!(app.cwd, dir);
    assert_eq!(app.sel, vec![dir.join("sub")]);
    app.go_back();
    assert_eq!(app.cwd, dir.join("sub"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn shift_selects_a_range_and_the_type_ahead_jumps() {
    let (dir, mut app) = setup("sel");
    app.select_only(0);
    app.select_to(2);
    assert_eq!(app.sel.len(), 3);
    let mut host = Host::default();
    app.key_inner(Key::Char('n'), KeyMods::default(), &mut host);
    assert_eq!(app.sel, vec![dir.join("note.txt")]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_typed_location_opens_that_folder() {
    let (dir, mut app) = setup("typed");
    app.go_typed("sub");
    assert_eq!(app.cwd, dir.join("sub"));
    app.go_typed("/definitely/not/here");
    assert_eq!(app.cwd, dir.join("sub"));
    assert!(app.fm.note.contains("No such folder"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn dropped_files_land_in_the_folder_shown() {
    let (dir, mut app) = setup("drop");
    let other = dir.join("sub").join("moved.txt");
    std::fs::write(&other, "y").unwrap();
    app.drop_files(vec![other.clone()], None, true);
    finish_jobs(&mut app, None);
    assert!(dir.join("moved.txt").exists() && other.exists(), "copied");
    assert_eq!(app.sel, vec![dir.join("moved.txt")], "the new file is selected");
    // and Undo takes the copy back (to the Trash)
    app.fm.undo_last();
    assert!(!dir.join("moved.txt").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_taken_name_asks_and_replace_keeps_the_old_one_in_the_trash() {
    let (dir, mut app) = setup("conflict");
    let incoming = dir.join("sub").join("note.txt");
    std::fs::write(&incoming, "new one").unwrap();
    app.start_job(jobs::Op::Copy, vec![incoming.clone()], dir.clone());
    // the job waits for an answer
    let t = Instant::now();
    while app.ask.is_none() && t.elapsed() < Duration::from_secs(5) {
        app.poll_jobs();
        std::thread::sleep(Duration::from_millis(5));
    }
    let ask = app.ask.take().expect("asked");
    assert_eq!(ask.name, "note.txt");
    ask.answer(jobs::Answer::Replace, false);
    finish_jobs(&mut app, None);
    assert_eq!(std::fs::read_to_string(dir.join("note.txt")).unwrap(), "new one");
    // undo: the copy goes, the old file comes back from the Trash
    app.fm.undo_last();
    assert_eq!(std::fs::read_to_string(dir.join("note.txt")).unwrap(), "x");
    // keep both makes a second name
    app.start_job(jobs::Op::Copy, vec![incoming], dir.clone());
    finish_jobs(&mut app, Some(jobs::Answer::KeepBoth));
    assert!(dir.join("note (2).txt").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_big_copy_runs_on_and_can_be_stopped() {
    let (dir, mut app) = setup("stop");
    let big = dir.join("sub").join("big.bin");
    std::fs::write(&big, vec![7u8; 64 << 20]).unwrap();
    app.start_job(jobs::Op::Copy, vec![big], dir.clone());
    app.jobs[0].cancel();
    finish_jobs(&mut app, None);
    assert!(!dir.join("big.bin").exists(), "a stopped copy leaves nothing half done");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_query_searches_the_folders_below() {
    let (dir, mut app) = setup("search");
    std::fs::write(dir.join("sub").join("alpha-deep.txt"), "x").unwrap();
    app.set_query("alpha".into());
    assert!(app.searching() && app.mixed());
    let t = Instant::now();
    while app.search.as_ref().is_some_and(|s| !s.done()) && t.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(5));
    }
    app.take_search();
    let names: Vec<String> = (0..app.shown_len()).map(|i| app.at(i).name.clone()).collect();
    assert_eq!(names, vec!["Alpha.md", "alpha-deep.txt"]);
    app.set_query(String::new());
    assert_eq!(app.shown_len(), 3, "back to the folder");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tabs_keep_their_own_folder() {
    let (dir, mut app) = setup("tabs");
    let mut host = Host::default();
    app.new_tab(dir.join("sub"));
    assert_eq!((app.tabs.len(), app.tab), (2, 1));
    assert_eq!(app.cwd, dir.join("sub"));
    app.switch_tab(0);
    assert_eq!(app.cwd, dir);
    assert_eq!(app.tab_title(1), "sub");
    app.close_tab(0, &mut host);
    assert_eq!(app.cwd, dir.join("sub"));
    assert!(!host.quit);
    app.close_tab(0, &mut host);
    assert!(host.quit, "the last tab closes the window");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn opened_files_show_in_recent() {
    let (dir, mut app) = setup("recent");
    memrium_de::recent::add(&dir.join("note.txt"), "test", "test %u");
    app.go(PathBuf::from(places::RECENT));
    assert!(app.recent() && app.pseudo() && app.mixed());
    assert!((0..app.shown_len()).any(|i| app.at(i).path == dir.join("note.txt")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rotating_turns_the_picture_in_its_file() {
    let (dir, _app) = setup("rot");
    let p = dir.join("wide.png");
    image::RgbaImage::from_pixel(4, 2, image::Rgba([9, 9, 9, 255])).save(&p).unwrap();
    assert_eq!(fsops::rotate_all(&[p.clone()], true), "Rotated 1 picture");
    assert_eq!(image::image_dimensions(&p).unwrap(), (2, 4));
    // the menu offers it for pictures, and the formats stay three
    let m = fsops::quick_edit_menu(&[p], memrium_de::files::cmd::CONVERT_BASE).unwrap();
    assert!(m.sub.iter().any(|i| i.id == memrium_de::files::cmd::ROTATE_LEFT));
    assert_eq!(m.sub.iter().filter(|i| i.label.starts_with("Convert to")).count(), 2, "own format left out");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------- drawn without a window

const W: usize = 1100;
const H: usize = 680;

struct Rig {
    app: FilesApp,
    state: State,
    gfx: Gfx,
    host: Host,
    dark: bool,
    buf: Vec<u8>,
}

impl Rig {
    fn new(app: FilesApp, dark: bool) -> Rig {
        Rig { app, state: State::default(), gfx: Gfx::new(), host: Host::default(), dark, buf: Vec::new() }
    }

    fn frame(&mut self) {
        let mut buf = vec![0u8; W * H * 4];
        for (i, px) in buf.chunks_exact_mut(4).enumerate() {
            let (x, y) = ((i % W) as f32 / W as f32, (i / W) as f32 / H as f32);
            let (r, g, b) = if self.dark { (0.10 + 0.10 * x, 0.12 + 0.08 * y, 0.22 + 0.12 * (1.0 - x)) } else { (0.62 + 0.25 * x, 0.74 + 0.1 * y, 0.95 - 0.1 * x) };
            px.copy_from_slice(&[(b * 255.0) as u8, (g * 255.0) as u8, (r * 255.0) as u8, 255]);
        }
        let mut canvas = Canvas::new(&mut buf, W, H);
        let mut painter = SoftDraw::new(&mut canvas, &mut self.gfx);
        let dt = self.state.begin_frame();
        let mut ui = Ui::new(&mut painter, &mut self.state, Style::new(self.dark, [0.20, 0.52, 0.95]), dt);
        self.app.tick(&mut self.host);
        self.app.draw(&mut ui, [W as f32, H as f32], &mut self.host);
        let seen = ui.edit_seen;
        self.state.end_frame(seen);
        self.buf = buf;
    }

    fn settle(&mut self, frames: usize) {
        for _ in 0..frames {
            std::thread::sleep(Duration::from_millis(16));
            self.frame();
        }
    }

    fn drag(&mut self, from: [f32; 2], to: [f32; 2]) {
        self.state.pointer_move(from);
        self.frame();
        self.state.pointer_down(from);
        self.frame();
        for k in 1..=8 {
            let t = k as f32 / 8.0;
            self.state.pointer_move([from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t]);
            self.frame();
        }
        self.state.pointer_up();
        self.frame();
    }

    fn shot(&self, name: &str) {
        let Ok(dir) = std::env::var("FILES_SHOT") else { return };
        let mut rgba = self.buf.clone();
        for px in rgba.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
        image::save_buffer(format!("{dir}/{name}.png"), &rgba, W as u32, H as u32, image::ColorType::Rgba8).unwrap();
    }
}

/// A folder with a few of everything, for the pictures.
fn sample(tag: &str) -> PathBuf {
    sandbox();
    let dir = std::env::temp_dir().join(format!("memrium-files-look-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for d in ["Projects", "Photos", "Music"] {
        std::fs::create_dir_all(dir.join(d)).unwrap();
    }
    std::fs::write(dir.join("notes.md"), "# Notes\n\nShopping:\n- bread\n- milk\n\nfn main() {\n    println!(\"hi\");\n}\n").unwrap();
    std::fs::write(dir.join("report.pdf"), "%PDF-1.4").unwrap();
    std::fs::write(dir.join("song.mp3"), "x").unwrap();
    std::fs::write(dir.join("archive.zip"), "x").unwrap();
    for (i, c) in [[230, 120, 80], [80, 160, 220], [120, 200, 120]].iter().enumerate() {
        let img = image::RgbaImage::from_fn(320, 200, |x, y| image::Rgba([(c[0] as u32 * x / 320) as u8, c[1], (c[2] as u32 * (200 - y) / 200) as u8, 255]));
        img.save(dir.join(format!("photo-{i}.png"))).unwrap();
    }
    dir.canonicalize().unwrap()
}

#[test]
fn the_dolphin_look_draws() {
    let dir = sample("dolphin");
    for dark in [false, true] {
        let mut app = FilesApp::new(dir.clone(), None);
        app.prefs = memrium_de::files::FilesPrefs::preset("Dolphin");
        app.prefs.info_panel = true;
        app.reload();
        app.new_tab(dir.join("Projects"));
        app.switch_tab(0);
        let mut rig = Rig::new(app, dark);
        rig.settle(10);
        // pick a picture so the Information panel has something to show
        let i = (0..rig.app.shown_len()).find(|&i| rig.app.at(i).name == "photo-1.png").unwrap();
        rig.app.select_only(i);
        rig.settle(40);
        rig.shot(if dark { "dolphin-dark" } else { "dolphin-light" });
        // a rubber band over the first row selects what it touches
        let list = rig.app.list_rect;
        rig.drag([list[0] + list[2] - 30.0, list[1] + list[3] - 10.0], [list[0] + 20.0, list[1] + 30.0]);
        assert!(rig.app.sel.len() >= 3, "the band selected {:?}", rig.app.sel);
        rig.app.prefs.view = ViewMode::Details;
        rig.settle(10);
        rig.shot(if dark { "dolphin-details-dark" } else { "dolphin-details-light" });
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_cards_draw() {
    let dir = sample("cards");
    let mut app = FilesApp::new(dir.clone(), None);
    app.prefs = memrium_de::files::FilesPrefs::preset("Dolphin");
    app.reload();
    let mut rig = Rig::new(app, false);
    rig.settle(5);
    // Quick Look on the text
    let i = (0..rig.app.shown_len()).find(|&i| rig.app.at(i).name == "notes.md").unwrap();
    rig.app.select_only(i);
    rig.app.toggle_quick();
    rig.settle(40);
    rig.shot("quicklook-text");
    let i = (0..rig.app.shown_len()).find(|&i| rig.app.at(i).name == "photo-0.png").unwrap();
    rig.app.select_only(i);
    rig.settle(30);
    rig.shot("quicklook-picture");
    rig.app.quick = false;
    // a copy that asks about a taken name, with its progress card under the question
    std::fs::write(dir.join("Projects").join("notes.md"), "other").unwrap();
    rig.app.start_job(jobs::Op::Copy, vec![dir.join("Projects").join("notes.md")], dir.clone());
    let t = Instant::now();
    while rig.app.ask.is_none() && t.elapsed() < Duration::from_secs(5) {
        rig.frame();
        std::thread::sleep(Duration::from_millis(10));
    }
    rig.settle(30);
    rig.shot("ask");
    if let Some(a) = rig.app.ask.take() {
        a.answer(jobs::Answer::Skip, true);
    }
    // the glass look
    rig.app.prefs = memrium_de::files::FilesPrefs::preset("Icons");
    rig.app.new_tab(dir.join("Photos"));
    rig.app.switch_tab(0);
    rig.settle(30);
    rig.shot("glass-tabs");
    let _ = std::fs::remove_dir_all(&dir);
}
