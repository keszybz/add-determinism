/* SPDX-License-Identifier: GPL-3.0-or-later */

mod config;
mod linkfiles;
mod fcontexts;

use anyhow::Result;

use add_determinism::{setup, simplelog};

fn print_selinux_contexts(config: &config::Config) -> Result<()> {
    let labels = config.selinux_labels.as_ref().unwrap();

    for path in config.inputs.iter() {
        let context = fcontexts::lookup_context(
            labels, config.root.as_deref(), path)?;
        println!("{} → {}", path.display(), context);
    }

    Ok(())
}

fn main() -> Result<()> {
    let config = config::Config::make()?;

    rlimit::increase_nofile_limit(u64::MAX)?;

    if config.print_selinux_contexts {
        print_selinux_contexts(&config)?;
    } else {
        let stats = linkfiles::process_inputs(&config)?;
        stats.summarize();
    }
    
    Ok(())
}
