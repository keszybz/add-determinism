/* SPDX-License-Identifier: GPL-3.0-or-later */

mod test_handlers;
mod test_invocation;

use add_determinism::simplelog;

use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Once;
use tempfile::TempDir;

static INIT: Once = Once::new();

fn init() {
    INIT.call_once(|| {
        simplelog::init(log::LevelFilter::Debug, true).unwrap();
    })
}

fn prepare_dir(path: &str) -> Result<(Box<TempDir>, Box<PathBuf>)> {
    init();

    let dir = TempDir::new()?;
    let input_path = dir.path().join(Path::new(path).file_name().unwrap());
    fs::copy(path, &input_path)?;
    Ok((Box::new(dir), Box::new(input_path)))
}
