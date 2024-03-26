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

    let mut inodes_seen = handlers::inodes_seen();

    for input_path in &config.args {
        handlers::process_file_or_dir(&config, &mut inodes_seen, input_path).unwrap_or_else(|err| {
            warn!("{}: failed to process: {}", input_path.display(), err);
            0
        });
    }

    Ok(())
}
