/* SPDX-License-Identifier: GPL-3.0-or-later */

use std::fs::File;
use std::os::linux::fs::MetadataExt;
use std::time;

use add_determinism::handlers;
use add_determinism::handlers::pyc;

use super::{prepare_dir, make_handler};

#[test]
fn test_adapters() {
    let (dir, input) = prepare_dir("tests/cases/adapters.cpython-312.pyc").unwrap();

    // We take the lazy step of creating an empty .py file here.
    // This is enough to be able to adjust the mtime.
    let py_file = File::options()
        .read(true)
        .write(true)
        .create_new(true)
        .open(dir.path().join("adapters.py"))
        .unwrap();

    let pyc = make_handler(111, false, pyc::PycZeroMtime::boxed).unwrap();

    assert!(pyc.filter(&*input).unwrap());

    let orig = input.metadata().unwrap();

    assert_eq!(pyc.process(&*input).unwrap(), handlers::ProcessResult::Replaced);

    let new = input.metadata().unwrap();
    // because of timestamp granularity, creation ts might be equal
    assert!(orig.created().unwrap() <= new.created().unwrap());
    assert_eq!(orig.modified().unwrap(), new.modified().unwrap());
    assert_ne!(orig.st_ino(), new.st_ino());

    let new2 = py_file.metadata().unwrap();
    assert_eq!(new2.modified().unwrap(), time::UNIX_EPOCH);
}
