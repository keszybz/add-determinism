/* SPDX-License-Identifier: GPL-3.0-or-later */

use std::io::Write;
use anyhow::Result;

fn main() -> Result<()> {
    let out_dir = std::env::var("OUT_DIR")?;
    let dest = std::path::Path::new(&out_dir).join("test_python_stdlib_cases.rs");
    let mut f = std::fs::File::create(dest)?;

    for (num, entry) in glob::glob("tests/cases/python_stdlib/*/*.pyc")?.enumerate() {
        write!(f, "
#[test]
fn test_python_stdlib_{}() {{
    test_python_stdlib_file(\"{}\");
}}
",
               num,
               entry?.display(),
        )?;
    }
    Ok(())
}
