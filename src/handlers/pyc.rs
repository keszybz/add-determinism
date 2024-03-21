/* SPDX-License-Identifier: GPL-3.0-or-later */

use anyhow::{Result, anyhow};
use log::debug;
use std::io::{BufWriter, Read, Write, Seek};
use std::path::Path;

use pyo3::prelude::*;

use crate::handlers::InputOutputHelper;
use crate::options;

pub fn filter(path: &Path) -> Result<bool> {
    Ok(path.extension().is_some_and(|x| x == "pyc"))
}

pub fn process(options: &options::Options, input_path: &Path) -> Result<bool> {
    let mut have_mod = false;
    let mut fp = InputOutputHelper::new(input_path)?;

    fp.finalize(have_mod)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_a() {
        assert!( filter(Path::new("/some/path/foobar.pyc")).unwrap());
        assert!(!filter(Path::new("/some/path/foobar.apyc")).unwrap());
        assert!( filter(Path::new("/some/path/foobar.opt-2.pyc")).unwrap());
        assert!(!filter(Path::new("/some/path/foobar")).unwrap());
        assert!(!filter(Path::new("/some/path/pyc")).unwrap());
        assert!(!filter(Path::new("/some/path/pyc_pyc")).unwrap());
        assert!(!filter(Path::new("/")).unwrap());
    }
}
