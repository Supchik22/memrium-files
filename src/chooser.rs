//! Chooser mode: Files as the desktop's file dialog. `memrium_portal` (the xdg-desktop-portal FileChooser backend) starts
//! it with the flags below; the answer is one JSON line on stdout, and nothing at all when the user cancels.
//!
//!   memrium_files --choose open|save|dir [--multiple] [--title T] [--accept LABEL] [--name FILE] [--folder DIR]
//!                 [--filter NAME\x1fg:GLOB\x1fm:MIME ...] [--current-filter N]

use memrium_de::fsops::Entry;
use std::io::Write;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Open,
    Save,
    Dir,
}

enum Rule {
    Glob(Vec<char>),
    Mime(String),
}

pub struct Filter {
    pub name: String,
    rules: Vec<Rule>,
}

impl Filter {
    fn matches(&self, file: &str) -> bool {
        let lower: Vec<char> = file.to_lowercase().chars().collect();
        self.rules.is_empty()
            || self.rules.iter().any(|r| match r {
                Rule::Glob(g) => glob(g, &lower),
                Rule::Mime(m) => mime_of(file).is_some_and(|have| match m.strip_suffix("/*") {
                    Some(kind) => have.split('/').next() == Some(kind),
                    None => have == m,
                }),
            })
    }
}

/// `*`, `?` and `[abc]` (the pattern is already lower case: the match ignores case, as people expect of `*.png`).
fn glob(p: &[char], s: &[char]) -> bool {
    match p.first() {
        None => s.is_empty(),
        Some('*') => (0..=s.len()).any(|i| glob(&p[1..], &s[i..])),
        Some('?') => !s.is_empty() && glob(&p[1..], &s[1..]),
        Some('[') => match p.iter().position(|&c| c == ']') {
            Some(end) if end > 1 => !s.is_empty() && p[1..end].contains(&s[0]) && glob(&p[end + 1..], &s[1..]),
            _ => s.first() == Some(&'[') && glob(&p[1..], &s[1..]),
        },
        Some(c) => s.first() == Some(c) && glob(&p[1..], &s[1..]),
    }
}

/// The MIME type of a file by its extension (the common ones: a filter by type is only a convenience).
fn mime_of(file: &str) -> Option<&'static str> {
    let ext = file.rsplit_once('.')?.1.to_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "tif" | "tiff" => "image/tiff",
        "ico" => "image/vnd.microsoft.icon",
        "avif" => "image/avif",
        "mp3" => "audio/mpeg",
        "ogg" | "oga" | "opus" => "audio/ogg",
        "flac" => "audio/flac",
        "wav" => "audio/x-wav",
        "m4a" => "audio/mp4",
        "mp4" | "m4v" => "video/mp4",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "avi" => "video/x-msvideo",
        "mov" => "video/quicktime",
        "txt" | "log" => "text/plain",
        "md" => "text/markdown",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "csv" => "text/csv",
        "xml" => "application/xml",
        "json" => "application/json",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "tar" => "application/x-tar",
        "7z" => "application/x-7z-compressed",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "odt" => "application/vnd.oasis.opendocument.text",
        _ => return None,
    })
}

pub struct Chooser {
    pub mode: Mode,
    pub multiple: bool,
    /// the label of the main button (the portal's `accept_label`)
    pub accept: String,
    pub filters: Vec<Filter>,
    pub filter: usize,
    /// the file name being typed (save)
    pub name: String,
    /// the user was already told the file exists and pressed the button again
    pub replace: bool,
    /// put the cursor in the name field on the first frame
    pub focus_name: bool,
    pub title: String,
}

impl Chooser {
    /// The chooser the flags ask for, and the folder to start in. `None` when it is the ordinary file manager.
    pub fn from_args(args: &[String]) -> (Option<Chooser>, Option<PathBuf>) {
        let mut mode = None;
        let mut c = Chooser { mode: Mode::Open, multiple: false, accept: String::new(), filters: Vec::new(), filter: 0, name: String::new(), replace: false, focus_name: false, title: String::new() };
        let mut start = None;
        let mut it = args.iter();
        while let Some(a) = it.next() {
            let mut value = || it.next().cloned().unwrap_or_default();
            match a.as_str() {
                "--choose" => {
                    mode = Some(match value().as_str() {
                        "save" => Mode::Save,
                        "dir" => Mode::Dir,
                        _ => Mode::Open,
                    })
                }
                "--multiple" => c.multiple = true,
                "--title" => c.title = value(),
                "--accept" => c.accept = value(),
                "--name" => c.name = value(),
                "--folder" => start = Some(PathBuf::from(value())).filter(|p| p.is_dir()),
                "--current-filter" => c.filter = value().parse().unwrap_or(0),
                "--filter" => {
                    let v = value();
                    let mut parts = v.split('\x1f');
                    let name = parts.next().unwrap_or("").to_string();
                    let rules = parts
                        .filter_map(|r| match r.split_once(':')? {
                            ("g", g) => Some(Rule::Glob(g.to_lowercase().chars().collect())),
                            ("m", m) => Some(Rule::Mime(m.to_lowercase())),
                            _ => None,
                        })
                        .collect();
                    c.filters.push(Filter { name, rules });
                }
                other if !other.starts_with("--") && start.is_none() => start = Some(PathBuf::from(other)).filter(|p| p.is_dir()),
                _ => {}
            }
        }
        let Some(mode) = mode else { return (None, start) };
        c.mode = mode;
        if c.filter >= c.filters.len() {
            c.filter = 0;
        }
        c.focus_name = mode == Mode::Save;
        if c.accept.is_empty() {
            c.accept = match mode {
                Mode::Save => "Save",
                Mode::Dir => "Select",
                Mode::Open => "Open",
            }
            .into();
        }
        if c.title.is_empty() {
            c.title = match mode {
                Mode::Save => "Save File",
                Mode::Dir => "Choose Folder",
                Mode::Open => "Open File",
            }
            .into();
        }
        (Some(c), start)
    }

    /// Whether the list shows `e`: folders always (to get around), files when they fit the filter, none when a folder is wanted.
    pub fn shows(&self, e: &Entry) -> bool {
        if e.is_dir {
            return true;
        }
        self.mode != Mode::Dir && self.filters.get(self.filter).map_or(true, |f| f.matches(&e.name))
    }

    /// Hands the answer to whoever started the dialog.
    pub fn emit(&self, paths: &[PathBuf]) {
        let list: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        let line = serde_json::json!({ "paths": list, "filter": self.filter });
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter(rules: Vec<Rule>) -> Filter {
        Filter { name: "t".into(), rules }
    }

    fn g(p: &str) -> Rule {
        Rule::Glob(p.to_lowercase().chars().collect())
    }

    #[test]
    fn globs_ignore_case_and_support_sets() {
        let f = filter(vec![g("*.png"), g("*.[jJ]pe?")]);
        assert!(f.matches("Photo.PNG") && f.matches("a.jpeg") && !f.matches("a.jpg") && !f.matches("png"));
    }

    #[test]
    fn mime_rules_use_the_extension() {
        let f = filter(vec![Rule::Mime("image/*".into()), Rule::Mime("application/pdf".into())]);
        assert!(f.matches("a.webp") && f.matches("b.PDF") && !f.matches("c.txt") && !f.matches("noext"));
        assert!(filter(vec![]).matches("anything"));
    }

    #[test]
    fn flags_make_a_save_dialog() {
        let args: Vec<String> = ["--choose", "save", "--name", "a.txt", "--filter", "Text\x1fg:*.txt", "--filter", "All\x1fg:*"].iter().map(|s| s.to_string()).collect();
        let (c, _) = Chooser::from_args(&args);
        let c = c.unwrap();
        assert!(c.mode == Mode::Save && c.name == "a.txt" && c.filters.len() == 2 && c.focus_name && c.accept == "Save");
        assert!(Chooser::from_args(&[]).0.is_none());
    }
}
