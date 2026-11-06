//! Drawing the window: the floating glass sidebar, the paper sheet with the toolbar row, the three views of a folder, the
//! status line, the file dialog's bar, the context menu and the Properties card.

use crate::chooser::Mode;
use crate::FilesApp;
use chebrium::app::Host;
use chebrium::chrome;
use chebrium::draw::Align;
use chebrium::popmenu::{self, Outcome};
use chebrium::ui::{inside, Cursor, Ui};
use chebrium::widgets::{self, fit, round_button, Round};
use memrium_de::files::{cmd, PathStyle, ViewMode};
use memrium_de::fsops::{self, SortBy};
use std::path::PathBuf;

const SIDE_W: f32 = 210.0;
const TOP_H: f32 = 52.0;
const HEAD_H: f32 = 28.0;
const ACT_H: f32 = 40.0;
const STATUS_H: f32 = 28.0;
const BAR_H: f32 = 56.0;

fn icon_button(ui: &mut Ui<'_>, cx: f32, cy: f32, icon: &str, enabled: bool) -> bool {
    round_button(ui, cx, cy, icon, Round::ink(30.0).glyph(17.0).enabled(enabled))
}

impl FilesApp {
    pub fn draw_window(&mut self, ui: &mut Ui<'_>, size: [f32; 2], host: &mut Host) {
        chrome::resize_borders(ui, host, size);
        let [w, h] = size;
        let (accent, ink, dark) = (ui.accent, ui.ink, ui.dark);
        let paper = if dark { [0.0; 3] } else { [1.0; 3] };
        let dialog = self.chooser.is_some();
        // an open menu or card is over everything: the window under it does not see the pointer
        let held = (self.menu.is_some()).then(|| popmenu::shield(ui));
        let veil = self.props.is_some().then(|| (ui.mouse, ui.click, ui.double, ui.scroll));
        if veil.is_some() {
            ui.mouse = [-1000.0; 2];
            ui.click = false;
            ui.double = false;
            ui.scroll = 0.0;
        }
        self.icons.begin_frame();

        let side_w = if self.prefs.sidebar { SIDE_W.min(self.prefs.sidebar_w.max(170.0) + 10.0) } else { 0.0 };
        let room = chrome::button_room(host);
        let act = if !dialog && self.prefs.toolbar_labels { ACT_H } else { 0.0 };
        let top = TOP_H + act;
        let foot = if dialog { BAR_H } else { 0.0 };
        let status_h = if self.prefs.status_bar || dialog { STATUS_H } else { 0.0 };
        let details = self.prefs.view == ViewMode::Details;
        let head = if details { HEAD_H } else { 0.0 };
        let list = [side_w, top + head, w - side_w, (h - top - head - status_h - foot).max(60.0)];
        self.list_rect = list;
        ui.fill([side_w, 0.0, w - side_w, h], 0.0, paper, if dark { 0.32 } else { 0.78 });

        // what files being dragged over the window would land on
        let drop_hot = host.drag_over.and_then(|p| self.drop_target(p));

        // F2 or a new folder: start renaming the selected entry
        if std::mem::take(&mut self.want_rename) {
            if let [p] = self.sel.as_slice() {
                if let Some(e) = self.entries.iter().find(|e| &e.path == p) {
                    self.rename_buf = e.name.clone();
                    self.renaming = Some(e.path.clone());
                    ui.begin_edit("rename", &e.name);
                }
            }
        }

        // ---- the entries
        let n = self.shown_len();
        let mut want_open = None;
        let mut renamed = None;
        let mut press_at = None;
        let usable_w = list[2] - 16.0 - widgets::BAR_ZONE;
        let (cell_w, cell_h) = match self.prefs.view {
            ViewMode::Details => (usable_w, self.prefs.row_h),
            ViewMode::Icons => (self.prefs.icon_size + 44.0, self.prefs.icon_size + 50.0),
            ViewMode::Compact => (236.0, self.prefs.row_h - 2.0),
        };
        let cols = if details { 1 } else { ((usable_w / cell_w).floor() as usize).max(1) };
        self.cols = cols;
        let rows = n.div_ceil(cols);
        if std::mem::take(&mut self.reveal) {
            if let Some(c) = self.cursor {
                widgets::reveal(&mut self.scroll, (c / cols) as f32 * cell_h, (c / cols + 1) as f32 * cell_h + 8.0, list[3]);
            }
        }
        self.rects.clear();
        let computer = self.computer();
        // columns from the right edge: Size (right aligned), Modified, Type; the name gets what is left
        let mut edge = w - 24.0 - widgets::BAR_ZONE;
        let wide = list[2] > 520.0 && !computer;
        let size_x = (wide && self.prefs.col_size).then(|| {
            let x = edge;
            edge -= 84.0;
            x
        });
        let date_x = (wide && self.prefs.col_date).then(|| {
            edge -= 150.0;
            edge
        });
        let kind_x = (wide && self.prefs.col_kind).then(|| {
            edge -= 120.0;
            edge
        });
        let area = widgets::scroll_area(ui, list, rows as f32 * cell_h + 8.0, &mut self.scroll);
        let square = self.prefs.selection_row && details;
        let radius = if self.prefs.selection_row { 4.0 } else { 9.0 };
        for row in area.rows(cell_h, rows) {
            for i in row * cols..((row + 1) * cols).min(n) {
                let e = self.at(i).clone();
                let r = [list[0] + 8.0 + (i % cols) as f32 * cell_w, area.y(4.0 + row as f32 * cell_h), cell_w, if details { cell_h - 2.0 } else { cell_h }];
                self.rects.push((i, r));
                let hov = area.hit(ui, r);
                let selected = self.sel.contains(&e.path);
                let cut = self.fm.cut && self.fm.clip.contains(&e.path);
                let a = if cut || e.hidden { 0.55 } else { 1.0 };
                let sel_r = [r[0] + 1.0, r[1], r[2] - 2.0, r[3]];
                if drop_hot.as_ref() == Some(&e.path) && e.is_dir {
                    ui.fill(sel_r, radius, accent, 0.4);
                } else {
                    widgets::list_row(ui, sel_r, radius, 30_000 + i as u64, hov, selected);
                }
                if square && selected {
                    ui.fill([r[0] + 1.0, r[1] + 6.0, 3.0, r[3] - 12.0], 1.5, accent, 1.0);
                } else if self.cursor == Some(i) && selected && !details {
                    // (the keyboard's row)
                }
                let editing = self.renaming.as_ref() == Some(&e.path);
                match self.prefs.view {
                    ViewMode::Icons => {
                        let side = self.prefs.icon_size * 0.92;
                        let (cx, cy) = (r[0] + r[2] / 2.0, r[1] + 8.0 + self.prefs.icon_size / 2.0);
                        self.paint_icon(ui, &e, cx, cy, side, true, a);
                        let ty = r[1] + self.prefs.icon_size + 30.0;
                        if editing {
                            let mut buf = self.rename_buf.clone();
                            if ui.text_field([r[0] + 2.0, ty - 14.0, r[2] - 4.0, 24.0], "rename", &mut buf) {
                                renamed = Some((e.path.clone(), buf.clone()));
                            }
                            self.rename_buf = buf;
                        } else {
                            let name = fit(ui, e.title(), 13.0, false, r[2] - 10.0);
                            ui.text_raw(r[0] + r[2] / 2.0, ty, &name, 13.0, selected, ink, 0.95 * a, Align::Center);
                        }
                    }
                    ViewMode::Compact | ViewMode::Details => {
                        let cy = r[1] + r[3] / 2.0;
                        let icon_px = if details { 22.0 } else { 20.0 };
                        self.paint_icon(ui, &e, r[0] + 22.0, cy, icon_px, false, a);
                        let name_max = if details { edge - r[0] - 56.0 } else { r[2] - 56.0 };
                        if editing {
                            let mut buf = self.rename_buf.clone();
                            if ui.text_field([r[0] + 40.0, r[1] + 1.0, name_max.min(360.0), r[3] - 2.0], "rename", &mut buf) {
                                renamed = Some((e.path.clone(), buf.clone()));
                            }
                            self.rename_buf = buf;
                        } else {
                            let name = fit(ui, e.title(), 13.5, false, name_max);
                            ui.text_raw(r[0] + 44.0, cy + 4.8, &name, 13.5, selected, ink, 0.95 * a, Align::Left);
                        }
                        if details {
                            if let Some(x) = kind_x {
                                ui.text(x, cy + 4.5, e.kind.label(), 12.0, false, ink, 0.55 * a, Align::Left);
                            }
                            if let Some(x) = date_x {
                                ui.text_raw(x, cy + 4.5, &fsops::format_time(e.modified), 12.0, false, ink, 0.55 * a, Align::Left);
                            }
                            if let (Some(x), false) = (size_x, e.is_dir) {
                                ui.text_raw(x, cy + 4.5, &fsops::human_size(e.size), 12.0, false, ink, 0.55 * a, Align::Right);
                            }
                        }
                    }
                }
                if ui.click && hov && !editing {
                    if ui.mods.shift && self.multi() {
                        self.select_to(i);
                    } else if ui.mods.ctrl && self.multi() {
                        self.toggle(i);
                    } else {
                        // pressing one of several selected entries keeps them (they can be dragged together)
                        if !(selected && self.sel.len() > 1) || ui.double {
                            self.select_only(i);
                        }
                        if ui.double {
                            want_open = Some(i);
                        } else {
                            press_at = Some(ui.mouse);
                        }
                    }
                    self.cursor = Some(i);
                }
            }
        }
        area.end(ui);
        if ui.click && inside(ui.mouse, list) && !self.rects.iter().any(|(_, r)| inside(ui.mouse, *r)) && self.renaming.is_none() && !ui.mods.ctrl {
            self.sel.clear();
        }
        if let Some(at) = press_at {
            self.pressed(at);
        }
        if let Some(i) = want_open {
            self.open(i);
        }
        if let Some((path, name)) = renamed {
            self.commit_rename(path, name);
        } else if self.renaming.is_some() && !ui.editing() {
            // Esc, or the field lost the pointer without a change
            self.renaming = None;
        }
        if n == 0 {
            let msg = if !self.error.is_empty() {
                self.error.clone()
            } else if !self.filter.is_empty() {
                chebrium::i18n::trf("Nothing matches “{}”", &[self.filter.clone()])
            } else if self.in_trash() {
                chebrium::i18n::tr("The Trash is empty").into_owned()
            } else {
                chebrium::i18n::tr("This folder is empty").into_owned()
            };
            ui.glyph(list[0] + list[2] / 2.0, list[1] + 70.0, 40.0, if self.error.is_empty() { "folder-open" } else { "info" }, ink, 0.25);
            ui.text_raw(list[0] + list[2] / 2.0, list[1] + 118.0, &msg, 14.0, false, ink, 0.55, Align::Center);
        }
        // pictures and file-type icons asked for are made now
        self.icons.end_frame();
        host.redraw |= self.icons.waiting;

        // ---- the column header
        if details {
            let top_h = top;
            ui.fill([side_w + 8.0, top_h + HEAD_H - 1.0, w - side_w - 16.0, 1.0], 0.0, ink, 0.10);
            let mut sort_to = None;
            let (sort, desc) = (self.prefs.sort, self.prefs.sort_desc);
            let mut head_cell = |ui: &mut Ui<'_>, label: &str, x: f32, align: Align, by: SortBy, width: f32| -> bool {
                let rx = if align == Align::Right { x - width } else { x };
                let hov = ui.hit([rx - 4.0, top_h, width + 8.0, HEAD_H]);
                ui.text(x, top_h + 18.5, label, 11.5, true, ink, if sort == by { 0.9 } else if hov { 0.8 } else { 0.5 }, align);
                if sort == by {
                    let tw = ui.measure(label, 11.5, true);
                    let ax = if align == Align::Right { x - tw - 10.0 } else { x + tw + 10.0 };
                    ui.glyph(ax, top_h + 14.0, 12.0, if desc { "chevron-down" } else { "chevron-up" }, ink, 0.8);
                }
                ui.click && hov
            };
            if head_cell(ui, "Name", side_w + 52.0, Align::Left, SortBy::Name, 80.0) {
                sort_to = Some(SortBy::Name);
            }
            if let Some(x) = kind_x {
                if head_cell(ui, "Type", x, Align::Left, SortBy::Kind, 60.0) {
                    sort_to = Some(SortBy::Kind);
                }
            }
            if let Some(x) = date_x {
                if head_cell(ui, "Modified", x, Align::Left, SortBy::Modified, 100.0) {
                    sort_to = Some(SortBy::Modified);
                }
            }
            if let Some(x) = size_x {
                if head_cell(ui, "Size", x, Align::Right, SortBy::Size, 60.0) {
                    sort_to = Some(SortBy::Size);
                }
            }
            if let Some(by) = sort_to {
                self.sort_by(by);
                self.save_prefs();
            }
        }

        // ---- the toolbar row: back/forward pill, the folder's name and path, a pill of actions
        let title = if let Some(c) = &self.chooser {
            c.title.clone()
        } else if computer {
            "Computer".to_string()
        } else {
            self.cwd.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "/".into())
        };
        if title != self.title {
            host.title = Some(title.clone());
            self.title = title;
        }
        let cy = TOP_H / 2.0;
        let left = if side_w > 0.0 { side_w } else { room[0] };
        let pill_x = left + 14.0;
        ui.fill([pill_x, cy - 16.0, 68.0, 32.0], 16.0, ink, 0.065);
        if icon_button(ui, pill_x + 18.0, cy, "chevron-left", self.can_back()) {
            self.go_back();
        }
        if icon_button(ui, pill_x + 50.0, cy, "chevron-right", self.can_forward()) {
            self.go_forward();
        }
        let sel_n = self.sel.len();
        let buttons: Vec<(&str, bool, u32)> = vec![
            ("arrow-up", self.can_up(), cmd::G_UP),
            ("refresh-cw", true, cmd::REFRESH),
            ("folder-plus", !computer, cmd::NEW_FOLDER),
            ("trash-2", sel_n > 0 && !dialog, if self.in_trash() { cmd::DELETE_FOREVER } else { cmd::TRASH }),
            ("search", true, crate::actions::FIND),
            (if self.prefs.show_hidden { "eye" } else { "eye-off" }, true, cmd::HIDDEN),
            (match self.prefs.view { ViewMode::Icons => "layout-grid", ViewMode::Details => "list", ViewMode::Compact => "columns-2" }, true, 0),
        ];
        let actions_w = buttons.len() as f32 * 32.0 + 8.0;
        let actions_x = (w - actions_w - 14.0 - room[1]).max(pill_x + 120.0);
        ui.fill([actions_x, cy - 16.0, actions_w, 32.0], 16.0, ink, 0.065);
        let mut chosen = None;
        for (i, (icon, on, id)) in buttons.iter().enumerate() {
            if icon_button(ui, actions_x + 20.0 + i as f32 * 32.0, cy, icon, *on) {
                chosen = Some(*id);
            }
        }
        // the filter's field sits left of the actions
        let mut title_right = actions_x - 12.0;
        if self.filter_open {
            let fw = 170.0f32.min((title_right - pill_x - 68.0 - 90.0).max(110.0));
            let fr = [actions_x - 12.0 - fw, cy - 14.0, fw, 28.0];
            title_right = fr[0] - 10.0;
            if std::mem::take(&mut self.want_filter) {
                ui.begin_edit("filter", &self.filter);
            }
            let mut buf = self.filter.clone();
            ui.text_field(fr, "filter", &mut buf);
            let live = ui.edit_text("filter").map(str::to_string).unwrap_or(buf);
            if live != self.filter {
                self.filter = live;
                self.apply_filter();
                self.scroll = 0.0;
            }
            if self.filter.is_empty() && !ui.editing() {
                self.filter_open = false;
            }
            if self.filter.is_empty() && ui.edit_text("filter").is_some_and(str::is_empty) {
                ui.text(fr[0] + 10.0, cy + 4.5, "Filter", 12.5, false, ink, 0.4, Align::Left);
            }
        }
        // the name and the path (each part of the path takes you there); a click on the name types a location
        let title_x = pill_x + 68.0 + 16.0;
        let title_w = (title_right - title_x).max(60.0);
        if let Some(text) = self.want_location.take() {
            ui.begin_edit("location", &text);
            self.locating = true;
        }
        if self.locating {
            let mut buf = self.cwd.to_string_lossy().to_string();
            if let Some(t) = ui.edit_text("location") {
                buf = t.to_string();
            }
            let typed = buf.clone();
            if ui.text_field([title_x - 6.0, cy - 15.0, title_w + 6.0, 30.0], "location", &mut buf) {
                self.locating = false;
                self.go_typed(&buf);
            } else if !ui.editing() {
                self.locating = false;
            }
            let _ = typed;
        } else {
            let name = fit(ui, &self.title, 15.0, true, title_w);
            ui.text_raw(title_x, cy - 1.5, &name, 15.0, true, ink, 0.95, Align::Left);
            let click = self.draw_path(ui, title_x, cy + 13.0, title_w);
            let name_r = [title_x - 4.0, 4.0, title_w, 26.0];
            if ui.hit(name_r) {
                ui.want_cursor(Cursor::Text);
                if ui.click {
                    self.want_location = Some(self.cwd.to_string_lossy().to_string());
                    host.redraw = true;
                }
            }
            if let Some(p) = click {
                self.go(p);
            }
        }
        match chosen {
            Some(0) => {
                self.prefs.view = match self.prefs.view {
                    ViewMode::Icons => ViewMode::Details,
                    ViewMode::Details => ViewMode::Compact,
                    ViewMode::Compact => ViewMode::Icons,
                };
                self.save_prefs();
            }
            Some(id) => self.command(id, host),
            None => {}
        }
        self.editing = ui.editing();
        // a press on the bare toolbar moves the window, twice maximises it
        let on_widget = ui.hit([pill_x, 0.0, 68.0, TOP_H]) || ui.hit([actions_x, 0.0, actions_w, TOP_H]) || ui.hit([title_x - 4.0, 0.0, title_w, TOP_H]);
        chrome::integrated(ui, host, size, TOP_H, on_widget);

        // ---- the labelled action row: New, Cut, Copy, Paste, Rename, Delete (the Explorer look)
        if act > 0.0 {
            ui.fill([side_w + 8.0, TOP_H + act - 1.0, w - side_w - 16.0, 1.0], 0.0, ink, 0.07);
            let (one, some) = (sel_n == 1, sel_n > 0);
            let acts: [(&str, &str, bool, u32); 6] = [
                ("folder-plus", "New", !computer, cmd::NEW_FOLDER),
                ("scissors", "Cut", some, cmd::CUT),
                ("copy", "Copy", some, cmd::COPY),
                ("clipboard-paste", "Paste", !computer && self.fm.can_paste(), cmd::PASTE),
                ("pencil", "Rename", one, cmd::RENAME),
                ("trash-2", "Delete", some, if self.in_trash() { cmd::DELETE_FOREVER } else { cmd::TRASH }),
            ];
            let mut x = side_w + 14.0;
            for (i, (icon, label, on, id)) in acts.into_iter().enumerate() {
                let tw = ui.measure(label, 12.5, false);
                let r = [x, TOP_H + 4.0, 38.0 + tw, act - 9.0];
                let hov = on && ui.hit(r);
                let wash = ui.ease(42_000 + i as u64, if hov { 1.0 } else { 0.0 }, 22.0);
                ui.fill(r, 9.0, ink, 0.09 * wash);
                ui.glyph(r[0] + 18.0, r[1] + r[3] / 2.0, 16.0, icon, if on { accent } else { ink }, if on { 1.0 } else { 0.28 });
                ui.text(r[0] + 32.0, r[1] + r[3] / 2.0 + 4.5, label, 12.5, false, ink, if on { 0.92 } else { 0.32 }, Align::Left);
                if ui.click && hov {
                    self.command(id, host);
                }
                x += r[2] + 4.0;
            }
        }

        // ---- status line
        let foot_y = h - foot;
        if status_h > 0.0 {
            let mut status = self.status();
            if let Some(c) = self.chooser.as_ref().filter(|c| c.replace) {
                status = chebrium::i18n::trf("“{}” already exists. Replace it?", &[c.name.trim().to_string()]);
            }
            let status = fit(ui, &status, 12.0, false, w - side_w - 130.0);
            ui.text_raw(side_w + 18.0, foot_y - 9.0, &status, 12.0, false, ink, 0.6, Align::Left);
            if self.prefs.view == ViewMode::Icons {
                for (i, (icon, id)) in [("minus", crate::actions::ZOOM_OUT), ("plus", crate::actions::ZOOM_IN)].into_iter().enumerate() {
                    if icon_button(ui, w - 56.0 + i as f32 * 30.0, foot_y - 14.0, icon, true) {
                        self.command(id, host);
                    }
                }
            }
        }

        // ---- the file dialog's bar: [name] [filter] [Cancel] [Open]
        if dialog {
            self.dialog_bar(ui, [side_w, foot_y, w - side_w, foot], host);
        }

        // ---- sidebar
        if self.prefs.sidebar {
            self.draw_sidebar(ui, h, side_w, drop_hot.as_ref(), host);
        } else {
            self.place_rects.clear();
        }

        // ---- the card of Properties
        if let Some(e) = self.props.clone() {
            let (mouse, click, double, scroll) = veil.unwrap_or(([-1000.0; 2], false, false, 0.0));
            ui.mouse = mouse;
            ui.click = click;
            ui.double = double;
            ui.scroll = scroll;
            ui.fill([0.0, 0.0, w, h], 0.0, [0.0; 3], 0.18);
            let lines = vec![
                ("Name", e.name.clone()),
                ("Type", chebrium::i18n::tr(e.kind.label()).into_owned()),
                ("Size", if e.is_dir { chebrium::i18n::tr("Folder").into_owned() } else { format!("{} ({} B)", fsops::human_size(e.size), e.size) }),
                ("Modified", fsops::format_time(e.modified)),
                ("Location", e.path.parent().map(|p| p.display().to_string()).unwrap_or_default()),
            ];
            let cw = 440.0f32.min(w - 40.0);
            let x = (w - cw) / 2.0;
            let y = (h / 2.0 - 100.0).max(60.0);
            let ch = widgets::facts_panel(ui, x, y, cw, &lines);
            if ui.click && !inside(ui.mouse, [x, y, cw, ch]) {
                self.props = None;
            }
            host.redraw = true;
        }

        // ---- the context menu, over everything
        if let (Some(m), Some(held)) = (self.menu.as_mut(), held) {
            let out = m.draw(ui, held, size);
            host.redraw |= m.animating();
            match out {
                Outcome::Open => {}
                Outcome::Chosen(id) => {
                    self.menu = None;
                    self.command(id, host);
                }
                Outcome::Closed => {
                    self.menu = None;
                    self.pending_convert = None;
                    self.menu_place = None;
                }
            }
            host.redraw = true;
        }
    }

    /// The icon of an entry: the theme's picture (or the thumbnail), else the Lucide glyph of its kind.
    fn paint_icon(&mut self, ui: &mut Ui<'_>, e: &fsops::Entry, cx: f32, cy: f32, side: f32, thumb: bool, a: f32) {
        match self.icons.get(e, thumb) {
            Some(i) => {
                let (pw, ph) = (i.pic.w as f32, i.pic.h as f32);
                ui.picture([cx - side / 2.0, cy - side / 2.0, side, side], &i.pic, [0.0, 0.0, pw, ph], a);
            }
            None => ui.glyph(cx, cy, (side * 0.8).max(16.0), e.kind.icon(), e.kind.tint(ui.accent), a),
        }
    }

    /// The path under the folder's name: each part takes you there (None unless one was pressed).
    fn draw_path(&mut self, ui: &mut Ui<'_>, x: f32, base: f32, max_w: f32) -> Option<PathBuf> {
        let ink = ui.ink;
        if self.computer() {
            return None;
        }
        let home = fsops::home();
        if self.prefs.path_style == PathStyle::Address {
            let t = fit(ui, &self.cwd.to_string_lossy(), 11.0, false, max_w);
            ui.text_raw(x, base, &t, 11.0, false, ink, 0.5, Align::Left);
            return None;
        }
        // Home › Documents › …, dropping the leftmost parts when too long
        let (root, rest) = match self.cwd.strip_prefix(&home) {
            Ok(r) => (home.clone(), r.to_path_buf()),
            Err(_) => (PathBuf::from("/"), self.cwd.strip_prefix("/").unwrap_or(&self.cwd).to_path_buf()),
        };
        let mut parts: Vec<(String, PathBuf)> = vec![(if root == home { "Home".into() } else { "/".into() }, root.clone())];
        let mut acc = root;
        for c in rest.components() {
            acc.push(c);
            parts.push((c.as_os_str().to_string_lossy().to_string(), acc.clone()));
        }
        let widths: Vec<f32> = parts.iter().map(|(n, _)| ui.measure_raw(n, 11.0, false) + 14.0).collect();
        let mut start = 0;
        while start + 1 < parts.len() && widths[start..].iter().sum::<f32>() + 10.0 * (parts.len() - start) as f32 > max_w {
            start += 1;
        }
        let mut px = x;
        let mut go = None;
        if start > 0 {
            ui.text_raw(px, base, "…", 11.0, false, ink, 0.5, Align::Left);
            px += 16.0;
        }
        for (i, (name, path)) in parts.iter().enumerate().skip(start) {
            let r = [px - 3.0, base - 12.0, widths[i], 16.0];
            let hov = ui.hit(r);
            if hov {
                ui.fill(r, 5.0, ink, 0.1);
            }
            let last = i + 1 == parts.len();
            ui.text_raw(px + 4.0, base, name, 11.0, false, ink, if hov || last { 0.8 } else { 0.5 }, Align::Left);
            if ui.click && hov && !last {
                go = Some(path.clone());
            }
            px += widths[i];
            if !last {
                ui.glyph(px - 3.0, base - 4.0, 10.0, "chevron-right", ink, 0.35);
                px += 6.0;
            }
        }
        go
    }

    fn status(&self) -> String {
        if !self.error.is_empty() {
            return self.error.clone();
        }
        if !self.fm.note.is_empty() {
            return self.fm.note.clone();
        }
        let sel = self.selected_entries();
        if sel.is_empty() {
            let dirs = self.shown.iter().filter(|&&i| self.entries[i].is_dir).count();
            let files = self.shown_len() - dirs;
            return chebrium::i18n::trf("{} folders, {} files", &[dirs.to_string(), files.to_string()]);
        }
        let size: u64 = sel.iter().filter(|e| !e.is_dir).map(|e| e.size).sum();
        if size > 0 {
            chebrium::i18n::trf("{} selected, {}", &[sel.len().to_string(), fsops::human_size(size)])
        } else {
            chebrium::i18n::trf("{} selected", &[sel.len().to_string()])
        }
    }

    fn dialog_bar(&mut self, ui: &mut Ui<'_>, bar: [f32; 4], host: &mut Host) {
        let (ink, foot) = (ui.ink, bar[3]);
        let can = self.can_accept();
        ui.fill([bar[0], bar[1], bar[2], 1.0], 0.0, ink, 0.10);
        let by = bar[1] + (foot - 32.0) / 2.0;
        let mut x = bar[0] + bar[2] - 16.0;
        let (mut cancel, mut ok, mut cycle) = (false, false, false);
        let (label, filter_name, many) = {
            let c = self.chooser.as_ref().unwrap();
            let label = if c.replace { "Replace".to_string() } else { c.accept.clone() };
            (label, c.filters.get(c.filter).map(|f| f.name.clone()), c.filters.len() > 1)
        };
        x -= 110.0;
        if can {
            ok = ui.button([x, by, 110.0, 32.0], &label, true);
        } else {
            ui.fill([x, by, 110.0, 32.0], 8.0, ink, 0.06);
            ui.text(x + 55.0, by + 20.5, &label, 12.5, true, ink, 0.3, Align::Center);
        }
        x -= 8.0 + 90.0;
        cancel |= ui.button([x, by, 90.0, 32.0], "Cancel", false);
        if let Some(name) = filter_name {
            x -= 8.0 + 190.0;
            let shown = fit(ui, &name, 12.5, false, 150.0);
            let r = [x, by, 190.0, 32.0];
            let hov = many && ui.hit(r);
            ui.fill(r, 8.0, ink, if hov { 0.14 } else { 0.08 });
            ui.text_raw(r[0] + 12.0, by + 20.5, &shown, 12.5, false, ink, 0.95, Align::Left);
            if many {
                ui.glyph(r[0] + r[2] - 16.0, by + 16.0, 14.0, "chevrons-up-down", ink, 0.6);
            }
            cycle = ui.click && hov;
        }
        let mut name_changed = false;
        if self.chooser.as_ref().is_some_and(|c| c.mode == Mode::Save) {
            let c = self.chooser.as_mut().unwrap();
            if c.focus_name {
                c.focus_name = false;
                ui.begin_edit("choose-name", &c.name);
            }
            let mut buf = c.name.clone();
            name_changed = ui.text_field([bar[0] + 16.0, by, (x - 12.0 - bar[0] - 16.0).max(120.0), 32.0], "choose-name", &mut buf);
            if name_changed {
                c.name = buf;
                c.replace = false;
            }
        }
        if cancel {
            host.quit = true;
        }
        if cycle {
            if let Some(c) = self.chooser.as_mut() {
                c.filter = (c.filter + 1) % c.filters.len();
            }
            self.reload();
        }
        if ok {
            self.accept();
        }
        host.redraw |= name_changed || cycle;
    }

    fn draw_sidebar(&mut self, ui: &mut Ui<'_>, h: f32, side_w: f32, drop_hot: Option<&PathBuf>, host: &mut Host) {
        let (ink, accent, dark) = (ui.ink, ui.accent, ui.dark);
        let paper = if dark { [0.0; 3] } else { [1.0; 3] };
        let panel = [8.0, 8.0, side_w - 16.0, h - 16.0];
        ui.fill(panel, 18.0, paper, if dark { 0.06 } else { 0.42 });
        ui.fill([panel[0], panel[1], panel[2], 1.0], 0.0, [1.0; 3], if dark { 0.05 } else { 0.5 });
        // the rows: (name, icon, path, rectangle), the two groups with their small headers
        let mut rows: Vec<(String, &'static str, PathBuf, [f32; 4])> = Vec::new();
        let mut headers: Vec<(&'static str, f32)> = Vec::new();
        let mut y = 56.0;
        let sidebar_headers = self.prefs.sidebar_headers;
        for (title, group) in [("PLACES", &self.places), ("SYSTEM", &self.system)] {
            if sidebar_headers {
                headers.push((title, y + 8.0));
                y += 24.0;
            }
            for p in group.iter() {
                rows.push((p.name.to_string(), p.icon, p.path.clone(), [16.0, y, side_w - 32.0, 32.0]));
                y += 34.0;
            }
            y += 6.0;
        }
        self.place_rects = rows.iter().map(|(_, _, p, r)| (p.clone(), *r)).collect();
        // one highlight glides to the place the window is in (the way the shell's menus do)
        match rows.iter().find(|(_, _, p, _)| *p == self.cwd) {
            Some((_, _, _, r)) => {
                for (s, v) in self.side_hl.iter_mut().zip(*r) {
                    s.target = v;
                }
                self.side_hl[4].target = 1.0;
                if !self.side_hl_init {
                    self.side_hl_init = true;
                    for s in self.side_hl.iter_mut() {
                        s.snap();
                    }
                    self.side_hl[4].x = 0.0;
                }
            }
            None => self.side_hl[4].target = 0.0,
        }
        let dt = ui.dt();
        for s in self.side_hl.iter_mut() {
            s.update(dt);
        }
        host.redraw |= self.side_hl.iter().any(|s| !s.at_rest());
        let a = self.side_hl[4].x.clamp(0.0, 1.0);
        let [hx, hy, hw, hh] = [self.side_hl[0].x, self.side_hl[1].x, self.side_hl[2].x.max(0.0), self.side_hl[3].x.max(0.0)];
        ui.fill([hx, hy, hw, hh], 10.0, ink, 0.09 * a);
        for (t, ty) in headers {
            ui.text(26.0, ty + 4.0, t, 10.5, true, ink, 0.4, Align::Left);
        }
        let mut go = None;
        for (i, (name, icon, path, r)) in rows.iter().enumerate() {
            let here = self.cwd == *path;
            let hov = ui.hit(*r);
            let wash = ui.ease(40_000 + i as u64, if hov { 1.0 } else { 0.0 }, 22.0);
            if drop_hot == Some(path) {
                ui.fill(*r, 10.0, accent, 0.4);
            } else {
                ui.fill(*r, 10.0, ink, 0.07 * wash * if here { 0.0 } else { 1.0 });
            }
            ui.glyph(r[0] + 18.0, r[1] + r[3] / 2.0, 16.0, icon, accent, 1.0);
            ui.text(r[0] + 40.0, r[1] + r[3] / 2.0 + 4.8, name, 13.5, here, ink, 0.95, Align::Left);
            if ui.click && hov {
                go = Some(path.clone());
            }
        }
        // the mounted disks at the bottom, each with a bar of how full it is
        const DEV_H: f32 = 46.0;
        let floor = y + 8.0;
        let fits = (((h - 16.0 - 8.0 - 20.0 - floor) / DEV_H).floor().max(0.0) as usize).min(self.devices.len());
        let mut dev_go = None;
        if fits > 0 {
            let top = h - 16.0 - 8.0 - fits as f32 * DEV_H;
            ui.text(24.0, top - 6.0, "DEVICES", 10.5, false, ink, 0.45, Align::Left);
            for (i, d) in self.devices.iter().take(fits).enumerate() {
                let r = [16.0, top + i as f32 * DEV_H, side_w - 32.0, DEV_H - 4.0];
                let hov = ui.hit(r);
                let wash = ui.ease(41_000 + i as u64, if hov { 1.0 } else { 0.0 }, 22.0);
                let here = self.cwd == d.path;
                ui.fill(r, 10.0, ink, if here { 0.09 } else { 0.07 * wash });
                ui.glyph(r[0] + 18.0, r[1] + 14.0, 16.0, "hard-drive", accent, 1.0);
                let label = fit(ui, &d.label, 13.0, here, r[2] - 50.0);
                ui.text_raw(r[0] + 40.0, r[1] + 18.8, &label, 13.0, here, ink, 0.95, Align::Left);
                let used = if d.total > 0 { (1.0 - d.free as f32 / d.total as f32).clamp(0.0, 1.0) } else { 0.0 };
                let bar = [r[0] + 12.0, r[1] + r[3] - 11.0, r[2] - 24.0, 5.0];
                ui.fill(bar, 2.5, ink, 0.12);
                if used > 0.0 {
                    let col = if used > 0.9 { [0.92, 0.3, 0.27] } else { accent };
                    ui.fill([bar[0], bar[1], (bar[2] * used).max(5.0), bar[3]], 2.5, col, 1.0);
                }
                if ui.click && hov {
                    dev_go = Some(d.path.clone());
                }
            }
        }
        if let Some(p) = go.or(dev_go) {
            self.go(p);
        }
    }
}
