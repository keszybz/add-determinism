/* SPDX-License-Identifier: GPL-3.0-or-later */

use std::os::linux::fs::MetadataExt;

use add_determinism::add_det::handlers;
use add_determinism::add_det::handlers::gzip;

use crate::common::prepare_dir;
use super::make_handler;

#[test]
fn test_empty() {
    let (_dir, input) = prepare_dir("tests/cases/empty.gz").unwrap();

    let handler = make_handler(1234567, false, gzip::Gzip::boxed).unwrap();

    assert!(handler.filter(&*input).unwrap());

    let orig = input.metadata().unwrap();

    assert_eq!(handler.process(&*input).unwrap(), handlers::ProcessResult::Replaced);

    let new = input.metadata().unwrap();
    assert_eq!(orig.modified().unwrap(), new.modified().unwrap());
    assert_ne!(orig.st_ino(), new.st_ino());

    assert_eq!(handler.process(&*input).unwrap(), handlers::ProcessResult::Noop);
}
