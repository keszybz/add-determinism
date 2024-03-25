/* SPDX-License-Identifier: GPL-3.0-or-later */

use std::os::linux::fs::MetadataExt;

use add_determinism::options::Options;
use add_determinism::handlers::ar;

use super::prepare_dir;

const OPTS: Options = Options{
    args: vec![],
    verbose: false,
    source_date_epoch: Some(111),
};

#[test]
fn test_libempty() {
    let (_dir, input) = prepare_dir("tests/cases/libempty.a").unwrap();

    assert!(ar::filter(&*input).unwrap());

    let orig = input.metadata().unwrap();

    assert_eq!(ar::process(&OPTS, &*input).unwrap(), false);

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

    assert_eq!(ar::process(&OPTS, &*input).unwrap(), true);

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

    assert!(ar::process(&OPTS, &*input).is_err());

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

    assert_eq!(ar::process(&OPTS, &*input).unwrap(), false);

    let new = input.metadata().unwrap();
    assert_eq!(orig.created().unwrap(), new.created().unwrap());
    assert_eq!(orig.modified().unwrap(), new.modified().unwrap());
    assert_eq!(orig.st_ino(), new.st_ino());
}
