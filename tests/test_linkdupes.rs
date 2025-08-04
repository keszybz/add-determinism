/* SPDX-License-Identifier: GPL-3.0-or-later */

mod common;

use std::fs;

use std::os::unix::fs::MetadataExt;

use add_determinism::linkdupes::{config, linkfiles};

#[test]
fn test_basic_linking() {
    let dir = common::empty_dir().unwrap();

    let a_txt = dir.path().join("a.txt");
    fs::write(&a_txt, b"").unwrap();
    let b_txt = dir.path().join("b.txt");
    fs::write(&b_txt, b"").unwrap();

    let mut config = config::Config::empty();
    config.inputs.push(dir.path().to_path_buf());
    config.ignore_mtime = true;

    let stats = linkfiles::process_inputs(&config).unwrap();
    assert_eq!(stats,
               linkfiles::Stats {
                   directories: 1,
                   files: 2,
                   candidate_files: 2,
                   files_read: 0,
                   files_linked: 1,
                   errors: 0,
                   bytes_linked: 0,
               });

    assert_eq!(a_txt.metadata().unwrap().nlink(), 2);
    assert_eq!(b_txt.metadata().unwrap().nlink(), 2);
}

#[test]
fn test_two_link_groups() {
    let dir = common::empty_dir().unwrap();

    let a_txt = dir.path().join("a.txt");
    let b_txt = dir.path().join("b.txt");
    fs::write(&a_txt, b"aaaa").unwrap();
    fs::hard_link(&a_txt, &b_txt).unwrap();

    let c_txt = dir.path().join("c.txt");
    let d_txt = dir.path().join("d.txt");
    fs::write(&c_txt, b"aaaa").unwrap();
    fs::hard_link(&c_txt, &d_txt).unwrap();

    let mut config = config::Config::empty();
    config.inputs.push(dir.path().to_path_buf());
    config.ignore_mtime = true;

    let stats = linkfiles::process_inputs(&config).unwrap();
    assert_eq!(linkfiles::Stats {
                   files_read: 0,
                   ..stats
               },
               linkfiles::Stats {
                   directories: 1,
                   files: 4,
                   candidate_files: 4,
                   files_read: 0,
                   files_linked: 2,
                   errors: 0,
                   bytes_linked: 8,
               });
    assert!(2 <= stats.files_read && stats.files_read <= 4); // depends on the comparison order

    assert_eq!(a_txt.metadata().unwrap().nlink(), 4);
    assert_eq!(b_txt.metadata().unwrap().nlink(), 4);
    assert_eq!(c_txt.metadata().unwrap().nlink(), 4);
    assert_eq!(d_txt.metadata().unwrap().nlink(), 4);
}

#[test]
fn test_different_modes() {
    let dir = common::empty_dir().unwrap();

    let a_txt = dir.path().join("a.txt");
    fs::write(&a_txt, b"").unwrap();
    let b_txt = dir.path().join("b.txt");
    fs::write(&b_txt, b"").unwrap();

    let mut perms = a_txt.metadata().unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&b_txt, perms).unwrap();

    let mut config = config::Config::empty();
    config.inputs.push(dir.path().to_path_buf());

    let stats = linkfiles::process_inputs(&config).unwrap();
    assert_eq!(stats,
               linkfiles::Stats {
                   directories: 1,
                   files: 2,
                   candidate_files: 2,
                   files_read: 0,
                   files_linked: 0,
                   errors: 0,
                   bytes_linked: 0,
               });

    assert_eq!(a_txt.metadata().unwrap().nlink(), 1);
    assert_eq!(b_txt.metadata().unwrap().nlink(), 1);
}
