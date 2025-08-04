/* SPDX-License-Identifier: GPL-3.0-or-later */

mod config;
mod linkfiles;

use anyhow::Result;

use add_determinism::{setup, simplelog};

fn main() -> Result<()> {
    let config = config::Config::make()?;

    rlimit::increase_nofile_limit(u64::MAX)?;

    let stats = linkfiles::process_inputs(&config)?;

    stats.summarize();
    
    Ok(())
}
