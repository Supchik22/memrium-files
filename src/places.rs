//! The places that are not plain folders of the sidebar: the bookmarks (GTK's `~/.config/gtk-3.0/bookmarks`, which GTK
//! programs and their file dialogs show too), the Recent view (the freedesktop `recently-used.xbel`), and how much room is
//! left on the disk of a folder (the Dolphin status bar).

use memrium_de::fsops::{self, Entry};
use std::path::{Path, PathBuf};

/// The pseudo folder of recently used files.
pub const RECENT: &str = "recent:";

pub fn is_recent(p: &Path) -> bool {
    p.as_os_str() == RECENT
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bookmark {
    pub path: PathBuf,
    /// the name to show (the folder's own name when the file gives none)
    pub label: String,
}

pub fn bookmarks_file() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| fsops::home().join(".config")).join("gtk-3.0").join("bookmarks")
}

/// The bookmarks of a GTK bookmarks text: `file:///path Label` lines (other schemes are left out: they are servers).
pub fn parse_bookmarks(text: &str) -> Vec<Bookmark> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            let (uri, label) = l.split_once(' ').unwrap_or((l, ""));
            let path = fsops::path_of_uri(uri)?;
            let label = if label.trim().is_empty() { path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.display().to_string()) } else { label.trim().to_string() };
            Some(Bookmark { path, label })
        })
        .collect()
}

/// The user's bookmarks of folders that are there.
pub fn bookmarks() -> Vec<Bookmark> {
    parse_bookmarks(&std::fs::read_to_string(bookmarks_file()).unwrap_or_default()).into_iter().filter(|b| b.path.is_dir()).collect()
}

/// `text` with `dir` added at the end (or left alone when it is there already).
pub fn with_bookmark(text: &str, dir: &Path) -> String {
    let uri = fsops::uri_of(dir);
    if text.lines().any(|l| l.split(' ').next() == Some(uri.as_str())) {
        return text.to_string();
    }
    let mut out = text.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&uri);
    out.push('\n');
    out
}

/// `text` without the line of `dir` (lines of other schemes kept as they are).
pub fn without_bookmark(text: &str, dir: &Path) -> String {
    let uri = fsops::uri_of(dir);
    let kept: Vec<&str> = text.lines().filter(|l| l.split(' ').next().is_none_or(|u| u != uri && fsops::path_of_uri(u).as_deref() != Some(dir))).collect();
    if kept.is_empty() {
        String::new()
    } else {
        kept.join("\n") + "\n"
    }
}

fn write_bookmarks(text: &str) -> std::io::Result<()> {
    let f = bookmarks_file();
    if let Some(d) = f.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = f.with_extension("new");
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, f)
}

pub fn add_bookmark(dir: &Path) -> std::io::Result<()> {
    let text = std::fs::read_to_string(bookmarks_file()).unwrap_or_default();
    write_bookmarks(&with_bookmark(&text, dir))
}

pub fn remove_bookmark(dir: &Path) -> std::io::Result<()> {
    let text = std::fs::read_to_string(bookmarks_file()).unwrap_or_default();
    write_bookmarks(&without_bookmark(&text, dir))
}

/// An entry of the list for any path (the Recent view and search results come from many folders).
pub fn entry_of(path: &Path) -> Option<Entry> {
    let md = std::fs::metadata(path).ok()?;
    let name = path.file_name()?.to_string_lossy().to_string();
    let kind = fsops::kind_of(&name, md.is_dir());
    let modified = md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    let app = if kind == fsops::Kind::App { memrium_de::apps::load_desktop(path).map(|a| (a.name, a.icon)) } else { None };
    Some(Entry { hidden: name.starts_with('.'), name, path: path.to_path_buf(), is_dir: md.is_dir(), size: md.len(), modified, kind, app })
}

/// The Recent view: files used lately, the newest first.
pub fn recent_entries() -> Vec<Entry> {
    memrium_de::recent::list(300).iter().filter_map(|(p, _)| entry_of(p)).filter(|e| !e.is_dir).collect()
}

/// (free, total) bytes of the disk a folder is on.
pub fn disk_space(dir: &Path) -> Option<(u64, u64)> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return None;
    }
    let unit = s.f_frsize as u64;
    Some((s.f_bavail as u64 * unit, s.f_blocks as u64 * unit))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bookmarks_are_read_added_and_removed() {
        let text = "file:///home/u/Desktop\nfile:///home/u/My%20Stuff Stuff\nsftp://host/x Server\n";
        let b = parse_bookmarks(text);
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].label, "Desktop");
        assert_eq!(b[1], Bookmark { path: "/home/u/My Stuff".into(), label: "Stuff".into() });
        let more = with_bookmark(text, Path::new("/home/u/new"));
        assert!(more.ends_with("file:///home/u/new\n"));
        assert_eq!(with_bookmark(&more, Path::new("/home/u/new")), more);
        let less = without_bookmark(&more, Path::new("/home/u/My Stuff"));
        assert_eq!(less, "file:///home/u/Desktop\nsftp://host/x Server\nfile:///home/u/new\n");
    }
}
