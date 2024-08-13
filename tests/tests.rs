/* SPDX-License-Identifier: GPL-3.0-or-later */

mod test_handlers;
mod test_invocation;

use add_determinism::simplelog;

use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

#[ctor::ctor]
fn init() {
    simplelog::init(log::LevelFilter::Debug, true).unwrap();
}

fn prepare_dir(path: &str) -> Result<(Box<TempDir>, Box<PathBuf>)> {
    let dir = TempDir::new()?;
    let input_path = dir.path().join(Path::new(path).file_name().unwrap());
    fs::copy(path, &input_path)?;
    Ok((Box::new(dir), Box::new(input_path)))
}
