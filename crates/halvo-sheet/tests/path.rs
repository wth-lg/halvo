mod common;

use std::path::Path;

use common::{link_dir, link_file, Scratch};
use halvo_sheet::{Cause, Error, Folder, PathRefusal};

fn refused(why: PathRefusal) -> Result<Vec<Vec<halvo_sheet::Value>>, Error> {
    Err(Error::Path(why))
}

#[test]
fn a_path_in_the_folder_reads_as_relative_and_as_absolute() {
    let scratch = Scratch::new("path-ok");
    scratch.write("folder/sub/a.csv", b"1\n");
    let folder = Folder::new(&scratch.path("folder"), common::locks()).expect("the folder opens");
    let rows = Ok(vec![vec![halvo_sheet::Value::Number(1.0)]]);
    assert_eq!(folder.read_source(Path::new("sub/a.csv")), rows);
    assert_eq!(folder.read_source(Path::new("./sub/a.csv")), rows);
    assert_eq!(folder.read_source(&scratch.path("folder/sub/a.csv")), rows);
}

#[test]
fn a_folder_given_through_a_link_takes_both_spellings() {
    let scratch = Scratch::new("path-folder-link");
    scratch.write("real/a.csv", b"1\n");
    link_dir(&scratch.path("real"), &scratch.path("alias"));
    let folder = Folder::new(&scratch.path("alias"), common::locks())
        .expect("the folder opens through a link");
    let rows = Ok(vec![vec![halvo_sheet::Value::Number(1.0)]]);
    let canonical = scratch.path("real").canonicalize().expect("canonical");
    assert_eq!(folder.read_source(&scratch.path("alias/a.csv")), rows);
    assert_eq!(folder.read_source(&canonical.join("a.csv")), rows);
}

#[cfg(unix)]
#[test]
fn a_spelling_of_the_folder_that_is_neither_the_given_nor_the_canonical_one_is_refused() {
    let scratch = Scratch::new("path-third-spelling");
    scratch.write("real/a.csv", b"1\n");
    link_dir(&scratch.path("real"), &scratch.path("alias"));
    link_dir(&scratch.path("real"), &scratch.path("other"));
    let folder = Folder::new(&scratch.path("alias"), common::locks())
        .expect("the folder opens through a link");
    assert_eq!(
        folder.read_source(&scratch.path("other/a.csv")),
        refused(PathRefusal::Outside)
    );
}

#[test]
fn a_path_out_of_the_folder_is_refused() {
    let scratch = Scratch::new("path-outside");
    scratch.write("folder/sub/a.csv", b"1\n");
    scratch.write("other.csv", b"1\n");
    let folder = Folder::new(&scratch.path("folder"), common::locks()).expect("the folder opens");
    assert_eq!(
        folder.read_source(&scratch.path("other.csv")),
        refused(PathRefusal::Outside)
    );
    assert_eq!(
        folder.read_source(Path::new("../other.csv")),
        refused(PathRefusal::Outside)
    );
    assert_eq!(
        folder.read_source(Path::new("sub/../sub/a.csv")),
        refused(PathRefusal::Outside)
    );
    assert_eq!(
        folder.read_source(&scratch.path("folder/../other.csv")),
        refused(PathRefusal::Outside)
    );
}

#[test]
fn a_link_anywhere_on_the_path_is_refused() {
    let scratch = Scratch::new("path-link");
    scratch.write("folder/inside.csv", b"1\n");
    scratch.write("folder/sub/a.csv", b"1\n");
    scratch.write("outside/a.csv", b"1\n");
    link_file(
        &scratch.path("folder/inside.csv"),
        &scratch.path("folder/to-inside.csv"),
    );
    link_file(
        &scratch.path("outside/a.csv"),
        &scratch.path("folder/to-outside.csv"),
    );
    link_dir(&scratch.path("outside"), &scratch.path("folder/out"));
    link_dir(&scratch.path("folder/sub"), &scratch.path("folder/same"));
    let folder = Folder::new(&scratch.path("folder"), common::locks()).expect("the folder opens");
    for rel in ["to-inside.csv", "to-outside.csv", "out/a.csv", "same/a.csv"] {
        let got = folder.read_source(Path::new(rel));
        assert_eq!(got, refused(PathRefusal::Link), "{rel}");
        assert_eq!(got.unwrap_err().cause(), Cause::StepFailed);
    }
}

#[test]
fn a_directory_and_the_folder_itself_are_refused() {
    let scratch = Scratch::new("path-dir");
    scratch.write("folder/sub/a.csv", b"1\n");
    let folder = Folder::new(&scratch.path("folder"), common::locks()).expect("the folder opens");
    assert_eq!(
        folder.read_source(Path::new("sub")),
        refused(PathRefusal::Directory)
    );
    assert_eq!(
        folder.read_source(Path::new("")),
        refused(PathRefusal::NotFile)
    );
    assert_eq!(
        folder.read_source(Path::new(".")),
        refused(PathRefusal::NotFile)
    );
    assert_eq!(
        folder.read_source(&scratch.path("folder")),
        refused(PathRefusal::NotFile)
    );
    assert_eq!(
        folder.read_source(Path::new("sub/a.csv/x")),
        refused(PathRefusal::NotFile)
    );
}

#[cfg(unix)]
#[test]
fn a_device_is_not_a_regular_file() {
    let folder = Folder::new(Path::new("/dev"), common::locks()).expect("the folder opens");
    assert_eq!(
        folder.read_source(Path::new("null")),
        refused(PathRefusal::NotFile)
    );
}

#[cfg(windows)]
#[test]
fn a_stream_name_is_refused_on_windows() {
    let scratch = Scratch::new("path-stream");
    scratch.write("folder/a.csv", b"1\n");
    let folder = Folder::new(&scratch.path("folder"), common::locks()).expect("the folder opens");
    assert_eq!(
        folder.read_source(Path::new("a.csv:hidden")),
        refused(PathRefusal::Name)
    );
}

#[test]
fn a_folder_that_is_no_directory_is_refused() {
    let scratch = Scratch::new("path-folder");
    let file = scratch.write("file.csv", b"1\n");
    assert_eq!(
        Folder::new(&file, common::locks()).unwrap_err(),
        Error::Folder
    );
    assert_eq!(
        Folder::new(&scratch.path("missing"), common::locks()).unwrap_err(),
        Error::Folder
    );
}

#[test]
fn a_missing_file_is_an_io_error() {
    let scratch = Scratch::new("path-missing");
    std::fs::create_dir_all(scratch.path("folder")).expect("make the folder");
    let folder = Folder::new(&scratch.path("folder"), common::locks()).expect("the folder opens");
    assert_eq!(
        folder.read_source(Path::new("missing.csv")),
        Err(Error::Io(std::io::ErrorKind::NotFound))
    );
}
