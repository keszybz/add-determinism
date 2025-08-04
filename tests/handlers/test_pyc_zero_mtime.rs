/* SPDX-License-Identifier: GPL-3.0-or-later */

use std::fs;
use std::time;
#[cfg(target_os = "linux")]
use std::os::linux::fs::MetadataExt as _;
#[cfg(target_os = "macos")]
use std::os::macos::fs::MetadataExt as _;

use add_determinism::add_det::handlers;

use crate::common::{invoke, prepare_dir};
use super::make_handler;

#[test]
fn test_adapters() {
    let (dir, input) = prepare_dir("tests/cases/adapters.cpython-312.pyc").unwrap();

    // We take the lazy step of creating an empty .py file here.
    // This is enough to be able to adjust the mtime.
    let py_file = fs::File::options()
        .read(true)
        .write(true)
        .create_new(true)
        .open(dir.path().join("adapters.py"))
        .unwrap();

    let pyc = make_handler(111, false, handlers::pyc::PycZeroMtime::boxed).unwrap();

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

fn test_pyc_zero_mtime(parallel: bool) {
    let (dir, _input) = prepare_dir("tests/cases/adapters.cpython-311~mtime.pyc").unwrap();

    let py_file = fs::File::options()
        .read(true)
        .write(true)
        .create_new(true)
        .open(dir.path().join("adapters.py"))
        .unwrap();

    let mut args = vec!["--handler=pyc-zero-mtime", dir.path().to_str().unwrap(), "-v"];
    if parallel {
        args.push("-j40");
    }

    let c = invoke(args);
    assert!(c.status.success());
    assert!(c.stderr.is_empty());

    let s = std::str::from_utf8(&c.stdout).unwrap();
    assert!(s.contains("Scanned 1 directories and 2 files,"));
    assert!(s.contains(" 1 modified (1 replaced + 0 rewritten),"));
    // TODO: should this be "2 modified"?

    assert_eq!(py_file.metadata().unwrap().modified().unwrap(), time::UNIX_EPOCH);

    let expected = fs::read("tests/cases/adapters.cpython-311~mtime.pyc.fixed").unwrap();
    let output = fs::read(dir.path().join("adapters.cpython-311~mtime.pyc")).unwrap();
    assert_eq!(output, expected);
}

#[test]
fn test_pyc_zero_mtime_serial() {
    test_pyc_zero_mtime(false);
}

#[test]
fn test_pyc_zero_mtime_parallel() {
    test_pyc_zero_mtime(true);
}
