/* SPDX-License-Identifier: GPL-3.0-or-later */

use std::fs::File;
use std::io::Read;
use std::os::linux::fs::MetadataExt;
use std::path::Path;

use add_determinism::options::Options;
use add_determinism::handlers::pyc;

use super::prepare_dir;

const OPTS: Options = Options{
    args: vec![],
    verbose: false,
    source_date_epoch: Some(111),
};

#[test]
fn test_verify_python3_pyc_3_12() {
    for p in [
        "tests/cases/adapters.cpython-312.pyc",
        "tests/cases/adapters.cpython-312.opt-1.pyc",
        "tests/cases/adapters.cpython-312.fixed.pyc",
    ] {
        let p = Path::new(p);

        let mut buf = [0; 4];
        File::open(p).unwrap().read_exact(&mut buf).unwrap();

        assert_eq!(pyc::verify_python3_pyc(&p, &buf).unwrap(), true);
    }
}

#[test]
fn test_adapters() {
    let (_dir, input) = prepare_dir("tests/cases/adapters.cpython-312.pyc").unwrap();

    assert!(pyc::filter(&*input).unwrap());

    let orig = input.metadata().unwrap();

    assert_eq!(pyc::process(&OPTS, &*input).unwrap(), true);

    let new = input.metadata().unwrap();
    // because of timestamp granularity, creation ts might be equal
    assert!(orig.created().unwrap() <= new.created().unwrap());
    assert_eq!(orig.modified().unwrap(), new.modified().unwrap());
    assert_ne!(orig.st_ino(), new.st_ino());
}

#[test]
fn test_adapters_opt_1() {
    let (_dir, input) = prepare_dir("tests/cases/adapters.cpython-312.opt-1.pyc").unwrap();

    assert!(pyc::filter(&*input).unwrap());

    let orig = input.metadata().unwrap();

    assert_eq!(pyc::process(&OPTS, &*input).unwrap(), true);

    let new = input.metadata().unwrap();
    // because of timestamp granularity, creation ts might be equal
    assert!(orig.created().unwrap() <= new.created().unwrap());
    assert_eq!(orig.modified().unwrap(), new.modified().unwrap());
    assert_ne!(orig.st_ino(), new.st_ino());
}


#[test]
fn test_testrelro_fixed() {
    let (_dir, input) = prepare_dir("tests/cases/adapters.cpython-312.fixed.pyc").unwrap();

    assert!(pyc::filter(&*input).unwrap());

    let orig = input.metadata().unwrap();

    assert_eq!(pyc::process(&OPTS, &*input).unwrap(), false);

    let new = input.metadata().unwrap();
    assert_eq!(orig.created().unwrap(), new.created().unwrap());
    assert_eq!(orig.modified().unwrap(), new.modified().unwrap());
    assert_eq!(orig.st_ino(), new.st_ino());
}
