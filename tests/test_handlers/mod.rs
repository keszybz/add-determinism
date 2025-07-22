/* SPDX-License-Identifier: GPL-3.0-or-later */

mod test_ar;
mod test_jar;
mod test_javadoc;
mod test_gzip;
mod test_pyc;
mod test_pyc_zero_mtime;

use anyhow::Result;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::rc::Rc;

use add_determinism::config;
use add_determinism::handlers;
use super::{init, prepare_dir};

fn make_handler(
    source_date_epoch: i64,
    check: bool,
    func: handlers::HandlerBoxed,
) -> Result<Box<dyn handlers::Processor>> {

    init();

    let cfg = Rc::new(config::Config::empty(source_date_epoch, check));
    let mut handler = func(&cfg);
    handler.initialize()?;
    Ok(handler)
}

struct Trivial {}

impl Trivial {
    pub fn boxed() -> Box<dyn handlers::Processor> {
        Box::new(Self {})
    }
}
impl handlers::Processor for Trivial {
    fn name(&self) -> &str {
        "trivial"
    }

    fn filter(&self, _path: &Path) -> Result<bool> {
        Ok(true)
    }

    fn process(&self, _input_path: &Path) -> Result<handlers::ProcessResult> {
        Ok(handlers::ProcessResult::Replaced)
    }
}

#[test]
fn test_input_output_helper_drop() {
    let (_dir, input) = prepare_dir("tests/cases/libempty.a").unwrap();

    let (mut helper, _) = handlers::InputOutputHelper::open(&*input, false, false).unwrap();
    helper.open_output(false).unwrap();

    let output_path = helper.output.as_ref().unwrap().path().to_path_buf();

    assert!(output_path.exists());
    drop(helper);
    assert!(!output_path.exists());
}

#[test]
fn test_input_output_helper_drop_no_file() {
    let (_dir, input) = prepare_dir("tests/cases/libempty.a").unwrap();

    let (mut helper, _) = handlers::InputOutputHelper::open(&*input, true, false).unwrap();
    helper.open_output(false).unwrap();

    let output_path = helper.output.as_ref().unwrap().path().to_path_buf();

    // tempfile.make() gives as a temporary file path, which we
    // summarilly ignore and open /dev/null instead. The file
    // specified this path doesn't exist. A bit of a hack.
    assert!(!output_path.exists());

    drop(helper);
    assert!(!output_path.exists());
}

fn stats(
    inodes_processed: u64,
    inodes_replaced: u64,
    inodes_rewritten: u64,
) -> handlers::Stats {
    handlers::Stats {
        directories: 1,
        files: 1,
        inodes_processed,
        inodes_replaced,
        inodes_rewritten,
        misunderstood: 0,
        errors: 0,
    }
}

#[test]
fn test_inode_map() {
    let (dir, _input) = prepare_dir("tests/cases/libempty.a").unwrap();

    let mut handlers = vec![ Trivial::boxed() ];
    let mut cache = handlers::inodes_seen();

    let mods = handlers::process_file_or_dir(&handlers, &mut cache, dir.path(), None);
    assert_eq!(mods, stats(1, 1, 0));

    let mods = handlers::process_file_or_dir(&handlers, &mut cache, dir.path(), None);
    assert_eq!(mods, stats(0, 0, 0));

    assert_eq!(cache.len(), 1);

    handlers.push(Trivial::boxed());

    let mods = handlers::process_file_or_dir(&handlers, &mut cache, dir.path(), None);
    assert_eq!(mods, stats(1, 1, 0));

    let mods = handlers::process_file_or_dir(&handlers, &mut cache, dir.path(), None);
    assert_eq!(mods, stats(0, 0, 0));

    assert_eq!(cache.len(), 1);
}

#[test]
fn test_inode_map_2() {
    let (dir, _input) = prepare_dir("tests/cases/testrelro.a").unwrap();

    let cfg = Rc::new(config::Config::empty(111, false));
    let ar = handlers::ar::Ar::boxed(&cfg);

    let handlers = vec![ar];
    let mut cache = handlers::inodes_seen();

    let mods = handlers::process_file_or_dir(&handlers, &mut cache, dir.path(), None);
    assert_eq!(mods, stats(1, 1, 0));

    let mods = handlers::process_file_or_dir(&handlers, &mut cache, dir.path(), None);
    // The file was already processed, so no change
    assert_eq!(mods, stats(0, 0, 0));

    // The inode changes because we rewrite the file
    assert_eq!(cache.len(), 2);
}

fn test_corpus_file(handler: Box<dyn handlers::Processor>, filename: &str) {
    let filename = Path::new(filename);
    let (_dir, input) = prepare_dir(filename.to_str().unwrap()).unwrap();

    let ext = filename.extension().unwrap().to_str().unwrap().to_owned();
    let fixed = filename.with_extension(ext + ".fixed");
    let have_mod = if fixed.exists() {
        handlers::ProcessResult::Replaced
    } else {
        handlers::ProcessResult::Noop
    };

    // If the filename starts with 'z', and we would rewrite the file anyway,
    // append garbage to test that this case is handled properly.
    if have_mod == handlers::ProcessResult::Replaced
        && filename.file_name().unwrap().to_str().unwrap().starts_with('z') {
            fs::File::options()
                .append(true)
                .open(&*input)
                .unwrap()
                .write(&[b'x', b'y', b'_', b'_'])
                .unwrap();
    }

    assert!(handler.filter(&*input).unwrap());
    assert_eq!(handler.process(&*input).unwrap(), have_mod);

    let mut data_expected = vec![];
    fs::File::open(
        if have_mod == handlers::ProcessResult::Replaced { &fixed } else { filename }
    ).unwrap()
        .read_to_end(&mut data_expected).unwrap();

    let mut data_output: Vec<u8> = vec![];
    fs::File::open(&*input).unwrap()
        .read_to_end(&mut data_output).unwrap();

    assert_eq!(data_output, data_expected);

    // Check that rerun does not result in any modifications
    assert_eq!(handler.process(&*input).unwrap(), handlers::ProcessResult::Noop);
}
