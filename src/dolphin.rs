//! The Dolphin layout (KDE's file manager) drawn the Chebrium way: a toolbar of pills (back/forward, the view modes, a
//! location bar of breadcrumbs that turns into a field on a click, search, the Information panel, the menu) over the
//! glass; a floating glass Places panel with its sections; the list on a paper sheet with the toolkit's rounded
//! selection; a status bar with the zoom slider and the free space of the disk; the Information panel (F11) with a
//! preview and the details of the selection.

use crate::view::Lay;
use crate::FilesApp;
use chebrium::app::Host;
use chebrium::chrome;
use chebrium::draw::Align;
use chebrium::ui::{Cursor, Ui};
use chebrium::widgets::{self, fit};
use memrium_de::files::{cmd, ViewMode};
use memrium_de::fsops;
use std::path::PathBuf;

pub const TOOL_H: f32 = 46.0;
pub const STATUS_H: f32 = 32.0;
pub const INFO_W: f32 = 260.0;
pub const FIND_H: f32 = 42.0;

/// The paper the list lies on (the window's glass shows around it), as in the glass look.
pub fn paper(dark: bool) -> ([f32; 3], f32) {
    if dark {
        ([0.0; 3], 0.32)
    } else {
        ([1.0; 3], 0.78)
    }
}

pub fn backgrounds(ui: &mut Ui<'_>, l: &Lay) {
    let (ink, dark) = (ui.ink, ui.dark);
    let (rgb, a) = paper(dark);
    // the toolbar and the Places panel stay on the window's glass; the view (search bar, header, list, status) is paper
    let x0 = l.list[0];
    let x1 = l.list[0] + l.list[2];
    let y0 = l.tabs[1] + l.tabs[3];
    ui.fill([x0, y0, l.w - x0, l.status[1] + l.status[3] - y0], 0.0, rgb, a);
    if l.info[2] > 0.0 {
        ui.fill([x1, y0 + 12.0, 1.0, l.info[3] - 24.0], 0.0, ink, 0.08);
    }
    // the floating glass card of the Places panel
    if l.side[2] > 0.0 {
        let card = [l.side[0] + 8.0, l.side[1] + 4.0, l.side[2] - 16.0, l.side[3] - 12.0];
        ui.fill(card, 16.0, rgb, if dark { 0.06 } else { 0.42 });
        ui.fill([card[0] + 12.0, card[1], card[2] - 24.0, 1.0], 0.0, [1.0; 3], if dark { 0.05 } else { 0.5 });
    }
}

/// An entry's or a place's box: the toolkit's rounded row (accent when selected, an eased wash under the pointer).
pub fn row(ui: &mut Ui<'_>, r: [f32; 4], key: u64, hov: bool, selected: bool, _cursor: bool) {
    widgets::list_row(ui, r, 9.0, key, hov, selected);
}

/// A pill behind a group of toolbar buttons.
fn pill(ui: &mut Ui<'_>, r: [f32; 4]) {
    let ink = ui.ink;
    ui.fill(r, r[3] / 2.0, ink, 0.065);
}

/// A round toolbar button inside a pill: a wash under the pointer, the accent when `on`.
fn tool(ui: &mut Ui<'_>, r: [f32; 4], glyph: &str, enabled: bool, on: bool, key: u64) -> bool {
    let (ink, accent) = (ui.ink, ui.accent);
    let hov = enabled && ui.hit(r);
    let wash = ui.ease(key, if hov { 1.0 } else { 0.0 }, 24.0);
    if on {
        ui.fill(r, r[3] / 2.0, accent, 0.22);
    } else if wash > 0.01 {
        ui.fill(r, r[3] / 2.0, ink, 0.08 * wash);
    }
    ui.glyph(r[0] + r[2] / 2.0, r[1] + r[3] / 2.0, 16.0, glyph, if on { accent } else { ink }, if enabled { 0.85 } else { 0.25 });
    if hov {
        ui.want_cursor(Cursor::Pointer);
    }
    hov && ui.click
}

impl FilesApp {
    pub fn dolphin_toolbar(&mut self, ui: &mut Ui<'_>, l: &Lay, host: &mut Host) {
        let ink = ui.ink;
        let room = chrome::button_room(host);
        let cy = TOOL_H / 2.0;
        let bh = 30.0;
        let by = cy - bh / 2.0;
        let mut x = room[0] + 10.0;
        pill(ui, [x - 1.0, by - 1.0, 2.0 * bh + 4.0, bh + 2.0]);
        let mut used = false;
        let hit = |ui: &Ui<'_>, r: [f32; 4]| ui.hit(r);
        let b = [x, by, bh, bh];
        used |= hit(ui, b);
        if tool(ui, b, "arrow-left", self.can_back(), false, 47_001) {
            self.go_back();
        }
        x += bh + 2.0;
        let b = [x, by, bh, bh];
        used |= hit(ui, b);
        if tool(ui, b, "arrow-right", self.can_forward(), false, 47_002) {
            self.go_forward();
        }
        x += bh + 12.0;
        // the view modes, side by side
        pill(ui, [x - 1.0, by - 1.0, 3.0 * (bh + 1.0) + 1.0, bh + 2.0]);
        let modes = [(ViewMode::Icons, "layout-grid", cmd::VIEW_ICONS), (ViewMode::Compact, "columns-2", cmd::VIEW_COMPACT), (ViewMode::Details, "list", cmd::VIEW_DETAILS)];
        let mut chosen = None;
        for (i, (m, g, id)) in modes.into_iter().enumerate() {
            let b = [x, by, bh, bh];
            used |= hit(ui, b);
            if tool(ui, b, g, true, self.prefs.view == m, 47_010 + i as u64) {
                chosen = Some(id);
            }
            x += bh + 1.0;
        }
        x += 9.0;
        // the right end: search, Information panel, the menu
        let mut rx = l.w - room[1] - 10.0 - bh;
        pill(ui, [rx - 2.0 * (bh + 2.0) - 1.0, by - 1.0, 3.0 * bh + 6.0, bh + 2.0]);
        let menu_r = [rx, by, bh, bh];
        used |= hit(ui, menu_r);
        let menu = tool(ui, menu_r, "menu", true, self.menu.is_some(), 47_020);
        rx -= bh + 2.0;
        let info_r = [rx, by, bh, bh];
        used |= hit(ui, info_r);
        let info = tool(ui, info_r, "panel-right", true, self.prefs.info_panel, 47_021);
        rx -= bh + 2.0;
        let find_r = [rx, by, bh, bh];
        used |= hit(ui, find_r);
        let find = tool(ui, find_r, "search", true, self.filter_open || !self.filter.is_empty(), 47_022);
        rx -= 10.0;
        // the location bar between them
        let bar = [x, by, (rx - x).max(80.0), bh];
        used |= ui.hit(bar);
        self.location_bar(ui, bar, host);
        if let Some(id) = chosen {
            self.command(id, host);
        }
        if find {
            if self.filter_open || !self.filter.is_empty() {
                self.filter_open = false;
                self.set_query(String::new());
            } else {
                self.command(crate::actions::FIND, host);
            }
        }
        if info {
            self.command(crate::actions::INFO_PANEL, host);
        }
        if menu {
            let at = [menu_r[0] + menu_r[2], menu_r[1] + menu_r[3] + 4.0];
            self.last_pointer = at;
            self.hamburger(at);
        }
        self.editing = ui.editing();
        let _ = ink;
        chrome::integrated(ui, host, [l.w, l.h], TOOL_H, used);
    }

    /// Dolphin's location bar: Home › Documents › … in a field; a press on its free part types a location.
    fn location_bar(&mut self, ui: &mut Ui<'_>, r: [f32; 4], host: &mut Host) {
        let (ink, dark) = (ui.ink, ui.dark);
        if let Some(text) = self.want_location.take() {
            ui.begin_edit("location", &text);
            self.locating = true;
        }
        if self.locating {
            let mut buf = self.location_text();
            if let Some(t) = ui.edit_text("location") {
                buf = t.to_string();
            }
            if ui.text_field(r, "location", &mut buf) {
                self.locating = false;
                self.go_typed(&buf);
            } else if !ui.editing() {
                self.locating = false;
            }
            return;
        }
        let _ = dark;
        ui.fill(r, r[3] / 2.0, ink, 0.065);
        let cy = r[1] + r[3] / 2.0;
        let mut x = r[0] + 12.0;
        let mut go = None;
        if self.pseudo() {
            let glyph = if self.computer() { "laptop" } else { "history" };
            ui.glyph(x + 8.0, cy, 15.0, glyph, ui.accent, 1.0);
            ui.text_raw(x + 24.0, cy + 4.5, &crate::place_title(&self.cwd), 13.0, false, ink, 0.9, Align::Left);
        } else {
            let parts = self.crumbs();
            let size = 13.0;
            let widths: Vec<f32> = parts.iter().map(|(n, _)| ui.measure_raw(n, size, false) + 16.0).collect();
            let sep = 16.0;
            let max_w = r[2] - 40.0;
            let mut start = 0;
            while start + 1 < parts.len() && widths[start..].iter().sum::<f32>() + sep * (parts.len() - start) as f32 > max_w {
                start += 1;
            }
            if start > 0 {
                ui.text_raw(x, cy + 4.5, "…", size, false, ink, 0.5, Align::Left);
                x += 18.0;
            }
            for (i, (name, path)) in parts.iter().enumerate().skip(start) {
                let last = i + 1 == parts.len();
                let cr = [x, r[1] + 4.0, widths[i], r[3] - 8.0];
                let hov = ui.hit(cr);
                if hov {
                    ui.fill(cr, cr[3] / 2.0, ink, 0.09);
                }
                if i == 0 && *path == fsops::home() {
                    ui.glyph(x + 10.0, cy, 14.0, "house", ink, 0.7);
                    ui.text_raw(x + 20.0, cy + 4.5, name, size, last, ink, 0.9, Align::Left);
                    x += 12.0;
                } else {
                    ui.text_raw(x + 8.0, cy + 4.5, name, size, last, ink, if last { 0.95 } else { 0.75 }, Align::Left);
                }
                if hov && ui.click {
                    go = Some(path.clone());
                }
                x += widths[i];
                if !last {
                    ui.glyph(x + 4.0, cy, 11.0, "chevron-right", ink, 0.4);
                    x += sep;
                }
            }
        }
        // the free part types a location
        let free = [x + 4.0, r[1], (r[0] + r[2] - x - 4.0).max(0.0), r[3]];
        if ui.hit(free) {
            ui.want_cursor(Cursor::Text);
            if ui.click {
                self.want_location = Some(self.location_text());
                host.redraw = true;
            }
        }
        if let Some(p) = go {
            if p != self.cwd {
                self.go(p);
            }
        }
    }

    /// The search bar under the toolbar: the field, what is searched, how far it got.
    pub fn dolphin_find(&mut self, ui: &mut Ui<'_>, l: &Lay) {
        let ink = ui.ink;
        let r = l.find;
        ui.fill([r[0], r[1] + r[3] - 1.0, r[2], 1.0], 0.0, ink, 0.08);
        let fw = (r[2] - 240.0).clamp(160.0, 520.0);
        let fr = [r[0] + 12.0, r[1] + 7.0, fw, r[3] - 14.0];
        self.query_field(ui, fr);
        let what = if self.pseudo() {
            chebrium::i18n::trf("in {}", &[crate::place_title(&self.cwd)])
        } else {
            chebrium::i18n::trf("in {} and its folders", &[crate::place_title(&self.cwd)])
        };
        let what = fit(ui, &what, 12.0, false, r[2] - fw - 40.0);
        ui.text_raw(fr[0] + fw + 14.0, r[1] + r[3] / 2.0 + 4.0, &what, 12.0, false, ink, 0.55, Align::Left);
    }

    pub fn dolphin_status(&mut self, ui: &mut Ui<'_>, l: &Lay, host: &mut Host) {
        let (ink, accent) = (ui.ink, ui.accent);
        let r = l.status;
        let cy = r[1] + r[3] / 2.0 + 0.5;
        let mut status = self.status();
        if let Some(c) = self.chooser.as_ref().filter(|c| c.replace) {
            status = chebrium::i18n::trf("“{}” already exists. Replace it?", &[c.name.trim().to_string()]);
        }
        // right: the free space of the disk, then the zoom
        let mut rx = r[0] + r[2] - 12.0;
        if let Some((free, total)) = self.space.filter(|_| r[2] > 520.0) {
            let sw = 150.0;
            let sr = [rx - sw, cy - 9.0, sw, 18.0];
            rx -= sw + 14.0;
            let used = if total > 0 { 1.0 - free as f32 / total as f32 } else { 0.0 };
            ui.fill(sr, 9.0, ink, 0.08);
            let col = if used > 0.9 { [0.86, 0.27, 0.24] } else { accent };
            ui.fill([sr[0], sr[1], (sr[2] * used).max(18.0), sr[3]], 9.0, col, 0.40);
            let t = chebrium::i18n::trf("{} free", &[fsops::human_size(free)]);
            ui.text_raw(sr[0] + sr[2] / 2.0, cy + 4.0, &t, 11.0, false, ink, 0.85, Align::Center);
        }
        if r[2] > 420.0 {
            let zw = 110.0;
            let zr = [rx - zw, cy - 4.0, zw, 8.0];
            rx -= zw + 34.0;
            ui.glyph(zr[0] - 14.0, cy, 13.0, "zoom-in", ink, 0.55);
            let icons = self.prefs.view == ViewMode::Icons;
            let (lo, hi) = if icons { (40.0, 128.0) } else { (24.0, 44.0) };
            let mut v = if icons { self.prefs.icon_size } else { self.prefs.row_h };
            let dragging = ui.slider_bare(zr, &mut v, lo, hi);
            let v = v.round();
            let old = if icons { self.prefs.icon_size } else { self.prefs.row_h };
            if (v - old).abs() >= 1.0 {
                if icons {
                    self.prefs.icon_size = v;
                } else {
                    self.prefs.row_h = v;
                }
                host.redraw = true;
            }
            if !dragging && (v - old).abs() >= 1.0 {
                self.save_prefs();
            }
        }
        let status = fit(ui, &status, 12.0, false, (rx - r[0] - 24.0).max(60.0));
        ui.text_raw(r[0] + 14.0, cy + 4.2, &status, 12.0, false, ink, 0.75, Align::Left);
    }

    pub fn dolphin_places(&mut self, ui: &mut Ui<'_>, side: [f32; 4], drop_hot: Option<&PathBuf>, host: &mut Host) {
        let (ink, accent) = (ui.ink, ui.accent);
        let row_h = 30.0;
        let mut y = side[1] + 14.0;
        let side = [side[0] + 8.0, side[1], side[2] - 16.0, side[3] - 8.0];
        let mut rows: Vec<(String, &'static str, PathBuf, [f32; 4])> = Vec::new();
        let mut heads: Vec<(String, f32)> = Vec::new();
        for (title, group) in self.side_groups() {
            if group.is_empty() {
                continue;
            }
            if self.prefs.sidebar_headers {
                heads.push((chebrium::i18n::tr(title).to_uppercase(), y + 16.0));
                y += 24.0;
            }
            for (name, glyph, path) in group {
                rows.push((name, glyph, path, [side[0] + 6.0, y, side[2] - 12.0, row_h]));
                y += row_h + 1.0;
            }
            y += 8.0;
        }
        // the disks, as Dolphin's Devices
        let devices: Vec<(String, PathBuf, f32)> = self.devices.iter().map(|d| (d.label.clone(), d.path.clone(), if d.total > 0 { 1.0 - d.free as f32 / d.total as f32 } else { 0.0 })).collect();
        if !devices.is_empty() && self.prefs.sidebar_headers {
            heads.push((chebrium::i18n::tr("Devices").to_uppercase(), y + 16.0));
            y += 24.0;
        }
        let dev_first = rows.len();
        for (label, path, _) in &devices {
            rows.push((label.clone(), "hard-drive", path.clone(), [side[0] + 6.0, y, side[2] - 12.0, row_h + 6.0]));
            y += row_h + 7.0;
        }
        self.place_rects = rows.iter().map(|(_, _, p, r)| (p.clone(), *r)).collect();
        for (t, ty) in heads {
            ui.text_raw(side[0] + 16.0, ty, &t, 10.5, true, ink, 0.4, Align::Left);
        }
        let mut go = None;
        ui.push_clip(side);
        for (i, (name, glyph, path, r)) in rows.iter().enumerate() {
            let here = self.cwd == *path;
            let hov = ui.hit(*r);
            if drop_hot == Some(path) {
                ui.fill(*r, 9.0, accent, 0.4);
            } else {
                row(ui, *r, 48_000 + i as u64, hov, here, false);
            }
            let dev = i >= dev_first;
            let icy = if dev { r[1] + 13.0 } else { r[1] + r[3] / 2.0 };
            ui.glyph(r[0] + 17.0, icy, 15.5, glyph, accent, 1.0);
            let label = fit(ui, name, 13.0, here, r[2] - 44.0);
            ui.text_raw(r[0] + 36.0, icy + 4.5, &label, 13.0, here, ink, 0.92, Align::Left);
            if dev {
                let used = devices[i - dev_first].2.clamp(0.0, 1.0);
                let bar = [r[0] + 36.0, r[1] + r[3] - 9.0, r[2] - 48.0, 3.0];
                ui.fill(bar, 1.5, ink, 0.12);
                let col = if used > 0.9 { [0.86, 0.27, 0.24] } else { accent };
                ui.fill([bar[0], bar[1], (bar[2] * used).max(3.0), bar[3]], 1.5, col, 0.9);
            }
            if hov && ui.click {
                go = Some(path.clone());
            }
        }
        ui.pop_clip();
        let _ = host;
        if let Some(p) = go {
            self.go(p);
        }
    }

    /// The Information panel: a big picture of the selection (or the folder) and what is known about it.
    pub fn info_panel(&mut self, ui: &mut Ui<'_>, r: [f32; 4], host: &mut Host) {
        let ink = ui.ink;
        let sel = self.selected_entries().into_iter().cloned().collect::<Vec<_>>();
        let e = match sel.as_slice() {
            [one] => Some(one.clone()),
            [] if !self.pseudo() => crate::places::entry_of(&self.cwd),
            _ => None,
        };
        let cx = r[0] + r[2] / 2.0;
        let mut y = r[1] + 20.0;
        let Some(e) = e else {
            if !sel.is_empty() {
                ui.glyph(cx, y + 70.0, 64.0, "copy", ui.accent, 0.8);
                let size: u64 = sel.iter().filter(|e| !e.is_dir).map(|e| e.size).sum();
                ui.text_raw(cx, y + 150.0, &chebrium::i18n::trf("{} items", &[sel.len().to_string()]), 14.0, true, ink, 0.95, Align::Center);
                ui.text_raw(cx, y + 172.0, &fsops::human_size(size), 12.0, false, ink, 0.6, Align::Center);
            }
            return;
        };
        let pw = r[2] - 32.0;
        let ph = 170.0;
        let preview = if e.is_dir { None } else { self.previews.get(&e) };
        let pic = preview.as_ref().and_then(|p| p.pic.clone()).or_else(|| self.icons.get(&e, true).map(|i| i.pic.clone()));
        match pic {
            Some(p) => {
                let (w0, h0) = (p.w as f32, p.h as f32);
                let k = (pw / w0).min(ph / h0).min(if preview.as_ref().is_some_and(|x| x.pic.is_some()) { 4.0 } else { 1.2 });
                let (dw, dh) = (w0 * k, h0 * k);
                ui.picture([cx - dw / 2.0, y + (ph - dh) / 2.0, dw, dh], &p, [0.0, 0.0, w0, h0], 1.0);
            }
            None => ui.glyph(cx, y + ph / 2.0, 72.0, e.kind.icon(), e.kind.tint(ui.accent), 1.0),
        }
        y += ph + 22.0;
        let name = fit(ui, e.title(), 14.0, true, r[2] - 24.0);
        ui.text_raw(cx, y, &name, 14.0, true, ink, 0.95, Align::Center);
        y += 26.0;
        let mut facts: Vec<(String, String)> = Vec::new();
        if e.is_dir {
            let p = self.previews.get(&e);
            facts.push(("Type".into(), chebrium::i18n::tr("Folder").into_owned()));
            if let Some(p) = p {
                facts.extend(p.facts.clone());
            }
        } else {
            facts.push(("Type".into(), memrium_de::mimeapps::describe(&memrium_de::mimeapps::mime_of(&e.path))));
            facts.push(("Size".into(), fsops::human_size(e.size)));
            if let Some(p) = &preview {
                facts.extend(p.facts.iter().filter(|(k, _)| k != "Size" && k != "Type").cloned());
            }
        }
        facts.push(("Modified".into(), fsops::format_time(e.modified)));
        if self.mixed() {
            facts.push(("Location".into(), e.path.parent().map(|p| p.display().to_string()).unwrap_or_default()));
        }
        let kx = r[0] + r[2] * 0.42;
        for (k, v) in facts {
            let key = chebrium::i18n::tr(&k).into_owned() + ":";
            ui.text_raw(kx - 8.0, y, &key, 12.0, false, ink, 0.55, Align::Right);
            let v = widgets::fit_start(ui, &v, 12.0, false, r[0] + r[2] - kx - 12.0);
            ui.text_raw(kx, y, &v, 12.0, false, ink, 0.9, Align::Left);
            y += 20.0;
            if y > r[1] + r[3] - 10.0 {
                break;
            }
        }
        let _ = host;
    }

    /// The menu behind the ≡ button: the view, the panels, the look.
    fn hamburger(&mut self, at: [f32; 2]) {
        use chebrium::appmenu::Item;
        let mut v = vec![
            Item::new(crate::actions::NEW_TAB, "New Tab").key("Ctrl+T").icon("plus"),
            Item::new(crate::actions::NEW_WINDOW, "New Window").key("Ctrl+N").icon("folder"),
            Item::new(cmd::NEW_FOLDER, "New Folder").key("Ctrl+Shift+N").icon("folder-plus").enabled(!self.pseudo()),
            Item::sep(),
            Item::new(cmd::UNDO, "Undo").key("Ctrl+Z").icon("undo-2").enabled(self.fm.can_undo()),
            Item::new(cmd::PASTE, "Paste").key("Ctrl+V").icon("clipboard-paste").enabled(!self.pseudo() && self.fm.can_paste()),
            Item::sep(),
        ];
        v.extend(crate::actions::view_menu(self));
        v.push(Item::sep());
        v.push(Item::new(cmd::SIDEBAR, "Show Places Panel").checked(self.prefs.sidebar));
        v.push(Item::new(cmd::STATUSBAR, "Show Status Bar").checked(self.prefs.status_bar));
        v.push(Item::parent("Settings", crate::actions::settings_menu(self)).icon("settings"));
        v.push(Item::sep());
        v.push(Item::new(cmd::TERMINAL, "Open Terminal here").icon("terminal").enabled(!self.pseudo()));
        self.menu = Some(chebrium::popmenu::PopMenu::new(at, v));
    }
}
