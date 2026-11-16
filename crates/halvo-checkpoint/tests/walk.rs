mod common;

use std::ffi::OsStr;
use std::io;
use std::path::Path;

use common::{link_dir, link_file, Scratch};
use halvo_checkpoint::{regular, Root, Walk};

#[test]
fn a_file_in_the_folder_is_reached_as_relative_and_as_absolute() {
    let scratch = Scratch::new("walk-ok");
    let file = scratch.write("folder/sub/a.xlsx", b"x");
    let root = Root::new(&scratch.path("folder")).expect("the folder opens");
    assert_eq!(root.file(Path::new("sub/a.xlsx")), Ok(file.clone()));
    assert_eq!(root.file(Path::new("./sub/a.xlsx")), Ok(file.clone()));
    assert_eq!(root.file(&file), Ok(file.clone()));
    let (dir, last) = root.parent(&file).expect("the parent");
    assert_eq!(
        (dir, last),
        (scratch.path("folder/sub"), OsStr::new("a.xlsx"))
    );
}

#[test]
fn a_path_outside_the_folder_or_with_a_parent_step_is_outside() {
    let scratch = Scratch::new("walk-outside");
    scratch.write("folder/a.xlsx", b"x");
    let other = scratch.write("other/a.xlsx", b"x");
    let root = Root::new(&scratch.path("folder")).expect("the folder opens");
    assert_eq!(root.file(&other), Err(Walk::Outside));
    assert_eq!(root.file(Path::new("../other/a.xlsx")), Err(Walk::Outside));
    let through = scratch.path("folder/../other/a.xlsx");
    assert_eq!(root.file(&through), Err(Walk::Outside));
    assert_eq!(root.parent(&other), Err(Walk::Outside));
}

#[test]
fn a_link_on_the_way_or_in_the_place_of_the_file_is_refused() {
    let scratch = Scratch::new("walk-link");
    scratch.write("folder/real/a.xlsx", b"x");
    let target = scratch.write("outside/b.xlsx", b"x");
    link_dir(&scratch.path("folder/real"), &scratch.path("folder/alias"));
    link_file(&target, &scratch.path("folder/b.xlsx"));
    let root = Root::new(&scratch.path("folder")).expect("the folder opens");
    assert_eq!(root.file(Path::new("alias/a.xlsx")), Err(Walk::Link));
    assert_eq!(root.parent(Path::new("alias/a.xlsx")), Err(Walk::Link));
    assert_eq!(root.file(Path::new("b.xlsx")), Err(Walk::Link));
    assert_eq!(regular(&scratch.path("folder/b.xlsx")), Err(Walk::Link));
}

#[test]
fn a_directory_the_folder_itself_and_an_absent_part_are_refused_by_their_kind() {
    let scratch = Scratch::new("walk-kind");
    scratch.write("folder/dir/a.xlsx", b"x");
    scratch.write("folder/plain", b"x");
    let root = Root::new(&scratch.path("folder")).expect("the folder opens");
    assert_eq!(root.file(Path::new("dir")), Err(Walk::Directory));
    assert_eq!(root.file(Path::new("")), Err(Walk::NotFile));
    assert_eq!(root.file(Path::new("plain/a.xlsx")), Err(Walk::NotFile));
    let absent = Err(Walk::Io(io::ErrorKind::NotFound));
    assert_eq!(root.file(Path::new("gone/a.xlsx")), absent);
    assert_eq!(root.file(Path::new("dir/gone.xlsx")), absent);
}

#[test]
fn a_folder_through_a_link_takes_both_spellings_and_a_file_is_no_folder() {
    let scratch = Scratch::new("walk-folder");
    let file = scratch.write("real/a.xlsx", b"x");
    link_dir(&scratch.path("real"), &scratch.path("alias"));
    let root = Root::new(&scratch.path("alias")).expect("the folder opens through a link");
    assert_eq!(root.dir(), scratch.path("real"));
    assert_eq!(root.file(&scratch.path("alias/a.xlsx")), Ok(file.clone()));
    assert_eq!(root.file(&file), Ok(file.clone()));
    assert_eq!(Root::new(&file).err(), Some(Walk::Folder));
    let absent = Root::new(&scratch.path("gone")).err();
    assert_eq!(absent, Some(Walk::Io(io::ErrorKind::NotFound)));
}

#[test]
fn the_debug_form_of_a_root_names_no_path() {
    let scratch = Scratch::new("walk-debug");
    let root = Root::new(&scratch.dir).expect("the folder opens");
    let shown = format!("{root:?}");
    assert!(
        !shown.contains("walk-debug") && !shown.contains('/'),
        "{shown}"
    );
}
