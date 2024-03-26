/* SPDX-License-Identifier: GPL-3.0-or-later */

mod handlers;
mod options;
mod simplelog;

use anyhow::Result;
use log::warn;

fn main() -> Result<()> {
    let config = match options::Config::make()? {
        None => { return Ok(()); },
        Some(some) => some
    };

    for input_path in &config.args {
        handlers::process_file_or_dir(&config, input_path).unwrap_or_else(|err| {
            warn!("Failed to process file: {}", err);
            0
        });
    }

    Ok(())
}
