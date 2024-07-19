/* SPDX-License-Identifier: GPL-3.0-or-later */

use std::env;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::process;

use crate::prepare_dir;

fn add_det_bin() -> PathBuf {
    let mut root = env::current_exe().unwrap()
        .parent()
        .expect("executable's directory")
        .to_path_buf();
    if root.ends_with("deps") {
        root.pop();
    }
    root.join("add-determinism")
}

fn invoke<I, S>(args: I) -> process::Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    process::Command::new(add_det_bin())
        .args(args)
        .output()
        .expect("failed to execute add-determinism")
}

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

    let c = invoke(["--check", "--handler=jar,javadoc,pyc", dir.path().to_str().unwrap()]);
    assert!(c.status.success());

    let c = invoke(["--check", "--handler=-ar", dir.path().to_str().unwrap()]);
    assert!(c.status.success());

    let c = invoke([dir.path()]);
    assert!(c.status.success());

    let c = invoke(["--check", dir.path().to_str().unwrap()]);
    assert!(c.status.success());

    let c = invoke(["--check", "--handler=ar", dir.path().to_str().unwrap()]);
    assert!(c.status.success());
}
