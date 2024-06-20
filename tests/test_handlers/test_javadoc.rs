/* SPDX-License-Identifier: GPL-3.0-or-later */

use std::fs;
use std::os::linux::fs::MetadataExt;

use add_determinism::handlers::javadoc;

use super::{prepare_dir, make_handler};

#[test]
fn test_javadoc_example() {
    let (_dir, input) = prepare_dir("tests/cases/javadoc-example.html").unwrap();

    let javadoc = make_handler(1704106800, javadoc::Javadoc::boxed).unwrap();

    assert!(javadoc.filter(&*input).unwrap());

    let orig = input.metadata().unwrap();

    assert_eq!(javadoc.process(&*input).unwrap(), true);

    let new = input.metadata().unwrap();
    // because of timestamp granularity, creation ts might be equal
    assert!(orig.created().unwrap() <= new.created().unwrap());
    assert_eq!(orig.modified().unwrap(), new.modified().unwrap());
    assert_ne!(orig.st_ino(), new.st_ino());

    assert_eq!(fs::read_to_string(&*input).unwrap(),
               fs::read_to_string("tests/cases/javadoc-example.fixed.html").unwrap());
}

#[test]
fn test_javadoc_fixed() {
    let (_dir, input) = prepare_dir("tests/cases/javadoc-example.fixed.html").unwrap();

    let javadoc = make_handler(1704106800, javadoc::Javadoc::boxed).unwrap();

    assert!(javadoc.filter(&*input).unwrap());

    let orig = input.metadata().unwrap();

    assert_eq!(javadoc.process(&*input).unwrap(), false);

    let new = input.metadata().unwrap();
    assert_eq!(orig.created().unwrap(), new.created().unwrap());
    assert_eq!(orig.modified().unwrap(), new.modified().unwrap());
    assert_eq!(orig.st_ino(), new.st_ino());
}

#[test]
fn test_invalid_utf8() {
    let (_dir, input) = prepare_dir("tests/cases/invalid-utf8.html").unwrap();

    let javadoc = make_handler(1704106800, javadoc::Javadoc::boxed).unwrap();

    assert!(javadoc.filter(&*input).unwrap());

    let orig = input.metadata().unwrap();

    assert_eq!(javadoc.process(&*input).unwrap(), false);

    let new = input.metadata().unwrap();
    assert_eq!(orig.created().unwrap(), new.created().unwrap());
    assert_eq!(orig.modified().unwrap(), new.modified().unwrap());
    assert_eq!(orig.st_ino(), new.st_ino());
}
