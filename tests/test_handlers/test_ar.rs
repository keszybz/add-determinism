use anyhow::Result;
use std::fs;
use std::os::linux::fs::MetadataExt;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

use add_determinism::options::Options;
use add_determinism::handlers::{FileProcess, ar};
use add_determinism::simplelog;

const OPTS: Options = Options{
    args: vec![],
    verbose: false,
    source_date_epoch: Some(111),
};

#[ctor::ctor]
fn init() {
    simplelog::init_with_level(log::LevelFilter::Debug).unwrap();
}

fn prepare_dir(path: &str) -> Result<(Box<TempDir>, Box<PathBuf>)> {
    let dir = TempDir::new()?;
    let input_path = dir.path().join(Path::new(path).file_name().unwrap());
    fs::copy(path, &input_path)?;
    Ok((Box::new(dir), Box::new(input_path)))
}

#[test]
fn test_libempty() {
    let (_dir, input) = prepare_dir("tests/cases/libempty.a").unwrap();

    assert!(ar::filter(&*input).unwrap());

    let orig = input.metadata().unwrap();

    assert_eq!(FileProcess::process(&OPTS, &*input, &ar::handler).unwrap(), false);

    let new = input.metadata().unwrap();
    assert_eq!(orig.created().unwrap(), new.created().unwrap());
    assert_eq!(orig.modified().unwrap(), new.modified().unwrap());
    assert_eq!(orig.st_ino(), new.st_ino());
}

#[test]
fn test_testrelro() {
    let (_dir, input) = prepare_dir("tests/cases/testrelro.a").unwrap();

    assert!(ar::filter(&*input).unwrap());

    let orig = input.metadata().unwrap();

    assert_eq!(FileProcess::process(&OPTS, &*input, &ar::handler).unwrap(), true);

    let new = input.metadata().unwrap();
    // because of timestamp granularity, creation ts might be equal
    assert!(orig.created().unwrap() <= new.created().unwrap());
    assert_eq!(orig.modified().unwrap(), new.modified().unwrap());
    assert_ne!(orig.st_ino(), new.st_ino());
}

#[test]
fn test_testrelro_c() {
    let (_dir, input) = prepare_dir("tests/cases/testrelro.c").unwrap();

    assert!(!ar::filter(&*input).unwrap());

    let orig = input.metadata().unwrap();

    assert!(FileProcess::process(&OPTS, &*input, &ar::handler).is_err());

    let new = input.metadata().unwrap();
    assert_eq!(orig.created().unwrap(), new.created().unwrap());
    assert_eq!(orig.modified().unwrap(), new.modified().unwrap());
    assert_eq!(orig.st_ino(), new.st_ino());
}

#[test]
fn test_testrelro_fixed() {
    let (_dir, input) = prepare_dir("tests/cases/testrelro.fixed.a").unwrap();

    assert!(ar::filter(&*input).unwrap());

    let orig = input.metadata().unwrap();

    assert_eq!(FileProcess::process(&OPTS, &*input, &ar::handler).unwrap(), false);

    let new = input.metadata().unwrap();
    assert_eq!(orig.created().unwrap(), new.created().unwrap());
    assert_eq!(orig.modified().unwrap(), new.modified().unwrap());
    assert_eq!(orig.st_ino(), new.st_ino());
}
