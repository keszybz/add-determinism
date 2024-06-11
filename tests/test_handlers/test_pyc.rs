/* SPDX-License-Identifier: GPL-3.0-or-later */

use std::fs;
use std::fs::File;
use std::io::Read;
use std::os::linux::fs::MetadataExt;
use std::path::Path;

use add_determinism::handlers::pyc;

use super::{prepare_dir, make_handler};

#[test]
fn test_pyc_python_version() {
    for p in [
        "tests/cases/adapters.cpython-312.pyc",
        "tests/cases/adapters.cpython-312.opt-1.pyc",
        "tests/cases/adapters.cpython-312~fixed.pyc",
    ] {
        let p = Path::new(p);

        let parser = pyc::PycParser::from_file(p, File::open(p).unwrap()).unwrap();
        assert_eq!(parser.version, (3, 12));
    }
}

#[test]
fn test_adapters() {
    let (_dir, input) = prepare_dir("tests/cases/adapters.cpython-312.pyc").unwrap();

    let pyc = make_handler(111, pyc::Pyc::boxed).unwrap();

    assert!(pyc.filter(&*input).unwrap());

    let orig = input.metadata().unwrap();

    assert_eq!(pyc.process(&*input).unwrap(), true);

    let new = input.metadata().unwrap();
    // because of timestamp granularity, creation ts might be equal
    assert!(orig.created().unwrap() <= new.created().unwrap());
    assert_eq!(orig.modified().unwrap(), new.modified().unwrap());
    assert_ne!(orig.st_ino(), new.st_ino());
}

#[test]
fn test_adapters_hardlinked() {
    let (_dir, input) = prepare_dir("tests/cases/adapters.cpython-312.pyc").unwrap();

    let pyc = make_handler(111, pyc::Pyc::boxed).unwrap();

    assert!(pyc.filter(&*input).unwrap());

    let orig = input.metadata().unwrap();

    fs::hard_link(&*input, (*input).with_extension("pyc.evenbetter")).unwrap();

    assert_eq!(pyc.process(&*input).unwrap(), true);

    let new = input.metadata().unwrap();
    assert_eq!(orig.created().unwrap(), new.created().unwrap());
    assert_eq!(orig.modified().unwrap(), new.modified().unwrap());
    assert_eq!(orig.st_ino(), new.st_ino());
}

#[test]
fn test_adapters_opt_1() {
    let (_dir, input) = prepare_dir("tests/cases/adapters.cpython-312.opt-1.pyc").unwrap();

    let pyc = make_handler(111, pyc::Pyc::boxed).unwrap();

    assert!(pyc.filter(&*input).unwrap());

    let orig = input.metadata().unwrap();

    assert_eq!(pyc.process(&*input).unwrap(), true);

    let new = input.metadata().unwrap();
    // because of timestamp granularity, creation ts might be equal
    assert!(orig.created().unwrap() <= new.created().unwrap());
    assert_eq!(orig.modified().unwrap(), new.modified().unwrap());
    assert_ne!(orig.st_ino(), new.st_ino());
}


#[test]
fn test_testrelro_fixed() {
    let (_dir, input) = prepare_dir("tests/cases/adapters.cpython-312~fixed.pyc").unwrap();

    let pyc = make_handler(111, pyc::Pyc::boxed).unwrap();

    assert!(pyc.filter(&*input).unwrap());

    let orig = input.metadata().unwrap();

    assert_eq!(pyc.process(&*input).unwrap(), false);

    let new = input.metadata().unwrap();
    assert_eq!(orig.created().unwrap(), new.created().unwrap());
    assert_eq!(orig.modified().unwrap(), new.modified().unwrap());
    assert_eq!(orig.st_ino(), new.st_ino());
}

fn test_python_stdlib_file(input_pyc: &str) {
    let input_pyc = Path::new(input_pyc);
    let (_dir, input) = prepare_dir(input_pyc.to_str().unwrap()).unwrap();

    let fixed_pyc = input_pyc.with_extension("pyc.fixed");
    let have_mod = fixed_pyc.exists();

    let pyc = make_handler(1717842014, pyc::Pyc::boxed).unwrap();

    assert!(pyc.filter(&*input).unwrap());
    assert_eq!(pyc.process(&*input).unwrap(), have_mod);

    let mut data_expected = vec![];
    File::open(
        if have_mod { &fixed_pyc } else { input_pyc }
    ).unwrap()
        .read_to_end(&mut data_expected).unwrap();

    let mut data_output: Vec<u8> = vec![];
    File::open(&*input).unwrap()
        .read_to_end(&mut data_output).unwrap();

    assert_eq!(data_output, data_expected);
}

#[test]
fn test_python_stdlib_file_1() {
    // Let's call test_python_stdlib_file() one manually for easier development.
    test_python_stdlib_file("tests/cases/python_stdlib/3.9/sunau.cpython-39.pyc");
}

include!(concat!(env!("OUT_DIR"), "/test_python_stdlib_cases.rs"));
