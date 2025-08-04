/* SPDX-License-Identifier: GPL-3.0-or-later */

mod common;

use common::{invoke, prepare_dir};

#[test]
fn test_help() {
    let c = invoke(["--help"]);
    assert!(c.status.success());
    assert!(c.stderr.is_empty());

    let s = std::str::from_utf8(&c.stdout).unwrap();
    assert!(s.contains("RPM buildroot helper to strip nondeterministic bits in files"));
    assert!(s.contains("--verbose"));
}

#[test]
fn test_help_many_args() {
    let c = invoke(["-h", "-j2", "--invalid"]);
    assert!(c.status.success());
    assert!(c.stderr.is_empty());

    let s = std::str::from_utf8(&c.stdout).unwrap();
    assert!(s.contains("RPM buildroot helper to strip nondeterministic bits in files"));
    assert!(s.contains("--check"));
}

#[test]
fn test_invocation() {
    let (dir, _input) = prepare_dir("tests/cases/testrelro.a").unwrap();

    let c = invoke([dir.path()]);
    assert!(c.status.success());
    assert!(c.stderr.is_empty());

    let s = std::str::from_utf8(&c.stdout).unwrap();
    assert!(s.contains("Scanned 1 directories and 1 files,"));
    assert!(s.contains(" 1 modified (1 replaced + 0 rewritten),"));
}

#[test]
fn test_invocation_j2() {
    let (dir, _input) = prepare_dir("tests/cases/testrelro.a").unwrap();

    let c = invoke(["-j2", dir.path().to_str().unwrap()]);
    assert!(c.status.success());
    assert!(c.stderr.is_empty());

    let s = std::str::from_utf8(&c.stdout).unwrap();
    assert!(s.contains("Scanned 1 directories and 1 files,"));
    assert!(s.contains(" 1 modified (1 replaced + 0 rewritten),"));
}

#[test]
fn test_invocation_j() {
    let (dir, _input) = prepare_dir("tests/cases/testrelro.a").unwrap();

    let c = invoke([dir.path().to_str().unwrap(), "-j"]);
    assert!(c.status.success());
    assert!(c.stderr.is_empty());

    let s = std::str::from_utf8(&c.stdout).unwrap();
    assert!(s.contains("Scanned 1 directories and 1 files,"));
    assert!(s.contains(" 1 modified (1 replaced + 0 rewritten),"));
}

#[test]
fn test_invocation_check() {
    let (dir, _input) = prepare_dir("tests/cases/testrelro.a").unwrap();

    let c = invoke(["--check", dir.path().to_str().unwrap()]);
    assert!(!c.status.success());

    let s = std::str::from_utf8(&c.stderr).unwrap();
    assert!(s.contains("Error: --check was specified, but some files would have been modified"));

    let s = std::str::from_utf8(&c.stdout).unwrap();
    assert!(s.contains("Scanned 1 directories and 1 files,"));
    assert!(s.contains(" 1 modified (1 replaced + 0 rewritten),"));
}

#[test]
fn test_invocation_check2() {
    let (dir, _input) = prepare_dir("tests/cases/testrelro.a").unwrap();

    let c = invoke(["--check", dir.path().to_str().unwrap()]);
    assert!(!c.status.success());

    let c = invoke(["--check", dir.path().to_str().unwrap(), "--handler", "ar"]);
    assert!(!c.status.success());

    let c = invoke(["--check", "--handler=javadoc,pyc", dir.path().to_str().unwrap()]);
    assert!(c.status.success());

    // $SOURCE_DATE_EPOCH is not set
    let c = invoke(["--check", "--handler=zip,javadoc,pyc", dir.path().to_str().unwrap()]);
    assert!(!c.status.success());

    let c = invoke(["--check", "--handler=-ar,-jar,-zip,-gzip", dir.path().to_str().unwrap()]);
    assert!(c.status.success());

    let c = invoke([dir.path()]);
    assert!(c.status.success());

    let c = invoke(["--check", dir.path().to_str().unwrap()]);
    assert!(c.status.success());

    let c = invoke(["--check", "--handler=ar", dir.path().to_str().unwrap()]);
    assert!(c.status.success());
}
