/* SPDX-License-Identifier: GPL-3.0-or-later */

mod handlers;
mod options;
mod simplelog;

use anyhow::{Result, anyhow};
use log::{debug, warn};
use std::env;
use std::path::Path;

fn brp_check() -> Result<()> {
    // env::current_exe() does readlink("/proc/self/exe"), which returns
    // the target binary, so we cannot use that.

    let arg0 = env::args().next().unwrap();

    let brp = Path::new(&arg0)
        .file_name()
        .ok_or(anyhow!("Exe path doesn't have a file name?"))?
        .to_str()
        .ok_or(anyhow!("Exe file name is not valid unicode"))?
        .starts_with("brp-");

    debug!("Running as {}… (brp={})", arg0, if brp { "true" } else { "false" });

    if brp {
        let build_root = env::var("RPM_BUILD_ROOT")
            .map_err(|_| anyhow!("RPM_BUILD_ROOT variable is not defined"))?;

        if build_root.is_empty() || build_root == "/" {
            return Err(anyhow!("RPM_BUILD_ROOT={:?} is not allowed", build_root));
        }
    }

    Ok(())
}

fn main() -> Result<()> {
    let config = match options::Config::make()? {
        None => { return Ok(()); },
        Some(some) => some
    };

    brp_check()?;

    let mut inodes_seen = handlers::inodes_seen();

    for input_path in &config.args {
        handlers::process_file_or_dir(&config, &mut inodes_seen, input_path).unwrap_or_else(|err| {
            warn!("{}: failed to process: {}", input_path.display(), err);
            0
        });
    }

    Ok(())
}
