/* SPDX-License-Identifier: GPL-3.0-or-later */

mod config;
mod linkfiles;
#[cfg(feature = "selinux")]
mod fcontexts;

#[cfg(not(feature = "selinux"))]
use anyhow::bail;
use anyhow::Result;

use add_determinism::{setup, simplelog};

#[cfg(feature = "selinux")]
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
        #[cfg(not(feature = "selinux"))]
        bail!("Compiled without support for SELinux");

        #[cfg(feature = "selinux")]
        print_selinux_contexts(&config)?;
    } else {
        let stats = linkfiles::process_inputs(&config)?;
        stats.summarize();
    }
    
    Ok(())
}
