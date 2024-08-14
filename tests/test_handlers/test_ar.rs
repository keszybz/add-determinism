/* SPDX-License-Identifier: GPL-3.0-or-later */

use std::fs;
use std::os::linux::fs::MetadataExt;
use std::rc::Rc;

use add_determinism::config;
use add_determinism::handlers;
use add_determinism::handlers::ar;

use super::{prepare_dir, make_handler, test_corpus_file};

#[test]
fn test_libempty() {
    let (_dir, input) = prepare_dir("tests/cases/libempty.a").unwrap();

    let ar = make_handler(111, false, ar::Ar::boxed).unwrap();

    assert!(ar.filter(&*input).unwrap());

    let orig = input.metadata().unwrap();

    assert_eq!(ar.process(&*input).unwrap(), handlers::ProcessResult::Noop);

    let new = input.metadata().unwrap();
    assert_eq!(orig.created().unwrap(), new.created().unwrap());
    assert_eq!(orig.modified().unwrap(), new.modified().unwrap());
    assert_eq!(orig.st_ino(), new.st_ino());
}

#[test]
fn test_testrelro() {
    let (_dir, input) = prepare_dir("tests/cases/testrelro.a").unwrap();

    let cfg = Rc::new(config::Config::empty(111, false));
    let ar = ar::Ar::boxed(&cfg);

    assert!(ar.filter(&*input).unwrap());

    let orig = input.metadata().unwrap();

    assert_eq!(ar.process(&*input).unwrap(), handlers::ProcessResult::Replaced);

    let new = input.metadata().unwrap();
    // because of timestamp granularity, creation ts might be equal
    assert!(orig.created().unwrap() <= new.created().unwrap());
    assert_eq!(orig.modified().unwrap(), new.modified().unwrap());
    assert_ne!(orig.st_ino(), new.st_ino());
}

#[test]
fn test_testrelro_check() {
    let (_dir, input) = prepare_dir("tests/cases/testrelro.a").unwrap();

    let cfg = Rc::new(config::Config::empty(111, true));
    let ar = ar::Ar::boxed(&cfg);

    assert!(ar.filter(&*input).unwrap());

    let orig = input.metadata().unwrap();

    assert_eq!(ar.process(&*input).unwrap(), handlers::ProcessResult::Replaced);

    let new = input.metadata().unwrap();
    assert_eq!(orig.created().unwrap(), new.created().unwrap());
    assert_eq!(orig.modified().unwrap(), new.modified().unwrap());
    assert_eq!(orig.st_ino(), new.st_ino());
}

#[test]
fn test_testrelro_hardlinked() {
    let (_dir, input) = prepare_dir("tests/cases/testrelro.a").unwrap();

    let ar = make_handler(111, false, ar::Ar::boxed).unwrap();

    assert!(ar.filter(&*input).unwrap());

    let orig = input.metadata().unwrap();

    fs::hard_link(&*input, (*input).with_extension("b")).unwrap();

    assert_eq!(ar.process(&*input).unwrap(), handlers::ProcessResult::Rewritten);

    let new = input.metadata().unwrap();
    assert_eq!(orig.created().unwrap(), new.created().unwrap());
    assert_eq!(orig.modified().unwrap(), new.modified().unwrap());
    assert_eq!(orig.st_ino(), new.st_ino());
}

#[test]
fn test_testrelro_hardlinked_check() {
    let (_dir, input) = prepare_dir("tests/cases/testrelro.a").unwrap();

    let ar = make_handler(111, true, ar::Ar::boxed).unwrap();

    assert!(ar.filter(&*input).unwrap());

    let orig = input.metadata().unwrap();

    fs::hard_link(&*input, (*input).with_extension("b")).unwrap();

    assert_eq!(ar.process(&*input).unwrap(), handlers::ProcessResult::Rewritten);

    let new = input.metadata().unwrap();
    assert_eq!(orig.created().unwrap(), new.created().unwrap());
    assert_eq!(orig.modified().unwrap(), new.modified().unwrap());
    assert_eq!(orig.st_ino(), new.st_ino());
}

#[test]
fn test_testrelro_c() {
    let (_dir, input) = prepare_dir("tests/cases/testrelro.c").unwrap();

    let ar = make_handler(111, false, ar::Ar::boxed).unwrap();

    assert!(!ar.filter(&*input).unwrap());

    let orig = input.metadata().unwrap();

    assert!(ar.process(&*input).is_err());

    let new = input.metadata().unwrap();
    assert_eq!(orig.created().unwrap(), new.created().unwrap());
    assert_eq!(orig.modified().unwrap(), new.modified().unwrap());
    assert_eq!(orig.st_ino(), new.st_ino());
}

#[test]
fn test_testrelro_fixed() {
    let (_dir, input) = prepare_dir("tests/cases/testrelro.fixed.a").unwrap();

    let ar = make_handler(111, false, ar::Ar::boxed).unwrap();

    assert!(ar.filter(&*input).unwrap());

    let orig = input.metadata().unwrap();

    assert_eq!(ar.process(&*input).unwrap(), handlers::ProcessResult::Noop);

    let new = input.metadata().unwrap();
    assert_eq!(orig.created().unwrap(), new.created().unwrap());
    assert_eq!(orig.modified().unwrap(), new.modified().unwrap());
    assert_eq!(orig.st_ino(), new.st_ino());
}

#[test]
fn test_libhsbase_compat_batteries() {
    // This is a test for buffered reads. When BufReader<File> is used,
    // we get unexpected short reads.
    // See https://github.com/keszybz/add-determinism/pull/14.

    let filename = "tests/cases/libHSbase-compat-batteries-0.12.3-EvvecFThiaEAGWq5U5Tpi9.a";

    let ar = make_handler(1717842014, false, ar::Ar::boxed).unwrap();
    test_corpus_file(ar, filename);
}
