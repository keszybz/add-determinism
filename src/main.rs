/* SPDX-License-Identifier: GPL-3.0-or-later */

mod handlers;
mod options;
mod simplelog;

use anyhow::Result;
use log::warn;

fn main() -> Result<()> {
    let options = options::Options::make()?;

    for input_path in &options.args {
        handlers::process_file_or_dir(&options, input_path).unwrap_or_else(|err| {
            warn!("Failed to process file: {}", err);
            0
        });
    }

    Ok(())
}
