/* SPDX-License-Identifier: GPL-3.0-or-later */

use std::fs;
use std::os::linux::fs::MetadataExt;
use std::rc::Rc;

use add_determinism::options;
use add_determinism::handlers::javadoc;

use super::prepare_dir;

#[test]
fn test_javadoc_example() {
    let (_dir, input) = prepare_dir("tests/cases/javadoc-example.html").unwrap();

    let cfg = Rc::new(options::Config::empty(1704106800));
    let javadoc = javadoc::Javadoc::boxed(&cfg);

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

    let cfg = Rc::new(options::Config::empty(1704106800));
    let javadoc = javadoc::Javadoc::boxed(&cfg);

    assert!(javadoc.filter(&*input).unwrap());

    let orig = input.metadata().unwrap();

    assert_eq!(javadoc.process(&*input).unwrap(), false);

    let new = input.metadata().unwrap();
    assert_eq!(orig.created().unwrap(), new.created().unwrap());
    assert_eq!(orig.modified().unwrap(), new.modified().unwrap());
    assert_eq!(orig.st_ino(), new.st_ino());
}
