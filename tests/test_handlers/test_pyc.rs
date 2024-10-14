/* SPDX-License-Identifier: GPL-3.0-or-later */

use std::io::Read;
use std::fs;
use std::fs::File;
use std::os::linux::fs::MetadataExt;
use std::path::Path;

use add_determinism::handlers;
use add_determinism::handlers::pyc;

use super::{prepare_dir, make_handler, test_corpus_file};

#[test]
fn test_pyc_header() {
    for (p, size) in [
        ("tests/cases/adapters.cpython-312.pyc", 16602),
        ("tests/cases/adapters.cpython-312.opt-1.pyc", 16602),
        ("tests/cases/adapters.cpython-312~fixed.pyc", 550),
    ] {
        let p = Path::new(p);

        let parser = pyc::PycParser::from_file(p, File::open(p).unwrap()).unwrap();
        assert_eq!(parser.version, (3, 12));
        assert_eq!(parser.py_content_hash(), None);
        assert_eq!(parser.py_content_mtime(), 1710422792);
        assert_eq!(parser.py_content_size(), size);
    }
}

#[test]
fn test_pyc_header_mtime_36() {
    let p = Path::new("tests/cases/adapters.cpython-36~mtime.pyc");

    let parser = pyc::PycParser::from_file(p, File::open(p).unwrap()).unwrap();
    assert_eq!(parser.version, (3, 6));
    assert_eq!(parser.py_content_hash(), None);
    assert_eq!(parser.py_content_mtime(), 1720707393);
    assert_eq!(parser.py_content_size(), 21);
}

#[test]
fn test_pyc_header_mtime_311() {
    let p = Path::new("tests/cases/adapters.cpython-311~mtime.pyc");

    let parser = pyc::PycParser::from_file(p, File::open(p).unwrap()).unwrap();
    assert_eq!(parser.version, (3, 11));
    assert_eq!(parser.py_content_hash(), None);
    assert_eq!(parser.py_content_mtime(), 1720707393);
    assert_eq!(parser.py_content_size(), 21);
}

#[test]
fn test_adapters() {
    let (_dir, input) = prepare_dir("tests/cases/adapters.cpython-312.pyc").unwrap();

    let pyc = make_handler(111, false, pyc::Pyc::boxed).unwrap();

    assert!(pyc.filter(&*input).unwrap());

    let orig = input.metadata().unwrap();

    assert_eq!(pyc.process(&*input).unwrap(), handlers::ProcessResult::Replaced);

    let new = input.metadata().unwrap();
    // because of timestamp granularity, creation ts might be equal
    assert!(orig.created().unwrap() <= new.created().unwrap());
    assert_eq!(orig.modified().unwrap(), new.modified().unwrap());
    assert_ne!(orig.st_ino(), new.st_ino());
}

#[test]
fn test_adapters_hardlinked() {
    let (_dir, input) = prepare_dir("tests/cases/adapters.cpython-312.pyc").unwrap();

    let pyc = make_handler(111, false, pyc::Pyc::boxed).unwrap();

    assert!(pyc.filter(&*input).unwrap());

    let orig = input.metadata().unwrap();

    fs::hard_link(&*input, (*input).with_extension("pyc.evenbetter")).unwrap();

    let mut data_expected = vec![];
    File::open("tests/cases/adapters.cpython-312~fixed.pyc")
        .unwrap()
        .read_to_end(&mut data_expected)
        .unwrap();

    assert_eq!(pyc.process(&*input).unwrap(), handlers::ProcessResult::Rewritten);

    let new = input.metadata().unwrap();
    assert_eq!(orig.created().unwrap(), new.created().unwrap());
    assert_eq!(orig.modified().unwrap(), new.modified().unwrap());
    assert_eq!(orig.st_ino(), new.st_ino());

    let mut data_output: Vec<u8> = vec![];
    File::open(&*input).unwrap()
        .read_to_end(&mut data_output).unwrap();

    assert_eq!(data_output, data_expected);
}

#[test]
fn test_adapters_opt_1() {
    let (_dir, input) = prepare_dir("tests/cases/adapters.cpython-312.opt-1.pyc").unwrap();

    let pyc = make_handler(111, false, pyc::Pyc::boxed).unwrap();

    assert!(pyc.filter(&*input).unwrap());

    let orig = input.metadata().unwrap();

    assert_eq!(pyc.process(&*input).unwrap(), handlers::ProcessResult::Replaced);

    let new = input.metadata().unwrap();
    // because of timestamp granularity, creation ts might be equal
    assert!(orig.created().unwrap() <= new.created().unwrap());
    assert_eq!(orig.modified().unwrap(), new.modified().unwrap());
    assert_ne!(orig.st_ino(), new.st_ino());
}


#[test]
fn test_adapters_fixed() {
    let (_dir, input) = prepare_dir("tests/cases/adapters.cpython-312~fixed.pyc").unwrap();

    let pyc = make_handler(111, false, pyc::Pyc::boxed).unwrap();

    assert!(pyc.filter(&*input).unwrap());

    let orig = input.metadata().unwrap();

    assert_eq!(pyc.process(&*input).unwrap(), handlers::ProcessResult::Noop);

    let new = input.metadata().unwrap();
    assert_eq!(orig.created().unwrap(), new.created().unwrap());
    assert_eq!(orig.modified().unwrap(), new.modified().unwrap());
    assert_eq!(orig.st_ino(), new.st_ino());
}

#[test]
fn test_pyc_file_with_dict() {
    let pyc = make_handler(12345678, false, pyc::Pyc::boxed).unwrap();
    test_corpus_file(pyc, "tests/cases/dicto.cpython-313.pyc");
}

#[test]
fn test_pyc_file_with_list() {
    let pyc = make_handler(12345679, false, pyc::Pyc::boxed).unwrap();
    test_corpus_file(pyc, "tests/cases/listo.cpython-313.pyc");
}

#[test]
fn test_python_stdlib_file_1() {
    // Let's call test_python_stdlib_file() once manually for easier development.
    test_python_stdlib_file("tests/cases/python_stdlib/3.9/sunau.cpython-39.pyc");
}

fn test_python_stdlib_file(filename: &str) {
    let pyc = make_handler(1717842014, false, pyc::Pyc::boxed).unwrap();
    test_corpus_file(pyc, filename);
}

include!(concat!(env!("OUT_DIR"), "/test_python_stdlib_cases.rs"));
