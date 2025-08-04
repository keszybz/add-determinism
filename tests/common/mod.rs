/* SPDX-License-Identifier: GPL-3.0-or-later */

use add_determinism::simplelog;

use anyhow::Result;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Once;
use tempfile::TempDir;

static INIT: Once = Once::new();

pub fn init() {
    INIT.call_once(|| {
        simplelog::init(log::LevelFilter::Debug, true).unwrap();
    })
}

pub fn prepare_dir(path: &str) -> Result<(Box<TempDir>, Box<PathBuf>)> {
    init();

    let dir = TempDir::new()?;
    let input_path = dir.path().join(Path::new(path).file_name().unwrap());
    fs::copy(path, &input_path)?;
    Ok((Box::new(dir), Box::new(input_path)))
}

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

pub fn invoke<I, S>(args: I) -> process::Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    process::Command::new(add_det_bin())
        .args(args)
        .env_remove("SOURCE_DATE_EPOCH")  // make sure that $SOURCE_DATE_EPOCH is
                                          // not inherited from the environment
        .output()
        .expect("failed to execute add-determinism")
}
