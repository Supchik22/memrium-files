use super::*;

fn setup(tag: &str) -> (PathBuf, FilesApp) {
    let dir = std::env::temp_dir().join(format!("memrium-files-app-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("note.txt"), "x").unwrap();
    std::fs::write(dir.join("Alpha.md"), "x").unwrap();
    let dir = dir.canonicalize().unwrap();
    (dir.clone(), FilesApp::new(dir, None))
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
    assert!(dir.join("moved.txt").exists() && other.exists(), "copied");
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
