mod test_ar;
mod test_javadoc;
mod test_pyc;

use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

use add_determinism::options;
use add_determinism::handlers;

fn prepare_dir(path: &str) -> Result<(Box<TempDir>, Box<PathBuf>)> {
    let dir = TempDir::new()?;
    let input_path = dir.path().join(Path::new(path).file_name().unwrap());
    fs::copy(path, &input_path)?;
    Ok((Box::new(dir), Box::new(input_path)))
}

pub fn filter(_path: &Path) -> Result<bool> {
    Ok(true)
}

pub fn process(_config: &options::Config, _input_path: &Path) -> Result<bool> {
    Ok(true)
}

pub const TRIVIAL: handlers::Processor = handlers::Processor {
    name: "trivial",
    filter,
    process,
};

#[test]
fn test_inode_map() {
    let (dir, _input) = prepare_dir("tests/cases/libempty.a").unwrap();

    let mut opts = options::Config::empty(1000000);
    opts.handlers.push(&TRIVIAL);

    let mut cache = handlers::inodes_seen();

    let mods = handlers::process_file_or_dir(&opts, &mut cache, dir.path()).unwrap();
    assert_eq!(mods, 1);

    let mods = handlers::process_file_or_dir(&opts, &mut cache, dir.path()).unwrap();
    assert_eq!(mods, 0);

    assert_eq!(cache.len(), 1);

    opts.handlers.push(&TRIVIAL);

    let mods = handlers::process_file_or_dir(&opts, &mut cache, dir.path()).unwrap();
    assert_eq!(mods, 1);

    let mods = handlers::process_file_or_dir(&opts, &mut cache, dir.path()).unwrap();
    assert_eq!(mods, 0);

    assert_eq!(cache.len(), 1);
}

#[test]
fn test_inode_map_2() {
    let (dir, _input) = prepare_dir("tests/cases/testrelro.a").unwrap();

    let mut opts = options::Config::empty(1000000);
    opts.handlers.push(&handlers::PROCESSORS[0]);

    let mut cache = handlers::inodes_seen();

    let mods = handlers::process_file_or_dir(&opts, &mut cache, dir.path()).unwrap();
    assert_eq!(mods, 1);

    let mods = handlers::process_file_or_dir(&opts, &mut cache, dir.path()).unwrap();
    assert_eq!(mods, 0); // The file was already processed, so no change

    // The inode changes because we rewrite the file
    assert_eq!(cache.len(), 2);
}
