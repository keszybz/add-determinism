/* SPDX-License-Identifier: GPL-3.0-or-later */

mod config;
mod linkfiles;

use anyhow::Result;

use crate::config::Config;

fn main() -> Result<()> {
    let config = Config::make()?;

    rlimit::increase_nofile_limit(u64::MAX)?;

    let stats = linkfiles::process_inputs(&config)?;

    stats.summarize();
    
    Ok(())
}
