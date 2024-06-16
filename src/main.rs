/* SPDX-License-Identifier: GPL-3.0-or-later */

mod handlers;
mod options;
mod multiprocess;
mod simplelog;

use anyhow::{Result, anyhow};
use log::{debug, info};
use std::env;
use std::path::Path;

fn brp_check(config: &options::Config) -> Result<()> {
    // env::current_exe() does readlink("/proc/self/exe"), which returns
    // the target binary, so we cannot use that.

    let arg0 = env::args().next().unwrap();

    debug!("Running as {arg0}… (brp={})", if config.brp { "true" } else { "false" });

    if config.brp {
        let build_root = env::var("RPM_BUILD_ROOT")
            .map_err(|_| anyhow!("RPM_BUILD_ROOT variable is not defined"))?;

        if build_root.is_empty() {
            return Err(anyhow!("Empty RPM_BUILD_ROOT is not allowed"));
        }

        let build_root_path = Path::new(&build_root).canonicalize()
            .map_err(|e| anyhow!("Cannot canonicalize RPM_BUILD_ROOT={:?}: {}", build_root, e))?;

        if build_root_path == Path::new("/") {
            return Err(anyhow!("RPM_BUILD_ROOT={:?} is not allowed", build_root));
        }

        for arg in &config.inputs {
            if !arg.starts_with(&build_root_path) {
                return Err(anyhow!("Path {:?} is outside of $RPM_BUILD_ROOT", arg));
            }
        }
    }

    Ok(())
}

fn main() -> Result<()> {
    let config = match options::Config::make()? {
        None => { return Ok(()); },
        Some(some) => some
    };


    brp_check(&config)?;

    let n_paths;

    if let Some(socket) = config.socket {
        debug!("Running as worker on socket {}", socket);
        return multiprocess::do_worker_work(config);

    } else if let Some(jobs) = config.jobs {
        debug!("Running as controller with {} workers", jobs);
        n_paths = multiprocess::Controller::do_work(config)?;

    } else {
        // We're not the controller
        n_paths = handlers::do_normal_work(config)?;
    }

    info!("Processed {} paths", n_paths);
    Ok(())
}
