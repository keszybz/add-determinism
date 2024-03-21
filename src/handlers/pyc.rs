/* SPDX-License-Identifier: GPL-3.0-or-later */

use anyhow::Result;
use log::debug;
use std::path::Path;

use pyo3::prelude::*;
use indoc::indoc;

use crate::handlers::InputOutputHelper;
use crate::options;

pub fn filter(path: &Path) -> Result<bool> {
    Ok(path.extension().is_some_and(|x| x == "pyc"))
}

pub fn process(_options: &options::Options, input_path: &Path) -> Result<bool> {
    let mut fp = InputOutputHelper::new(input_path)?;

    pyo3::prepare_freethreaded_python();

    Python::with_gil(|py| {
        let fun: Py<PyAny> = PyModule::from_code(
            py, indoc! {"
            from marshalparser.marshalparser import MarshalParser
            from pathlib import Path

            def fix(file):
                parser = MarshalParser(Path(file))
                parser.parse()
                parser.clear_unused_ref_flags(overwrite=False)"},
            "",
            "",
        )?
        .getattr("fix")?
        .into();

        debug!("{}: Calling python fix()", fp.input_path.display());
        let path = fp.input_path.to_str().unwrap();
        fun.call1(py, (path,))
    })?;

    // MarshalParser creates a file "input.fixed.pyc" if changes were made.
    // If it exists, assume modifications have been made.
    fp.output_path = Some(fp.input_path.with_extension("fixed.pyc"));

    fp.finalize(true)
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
