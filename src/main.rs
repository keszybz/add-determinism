/* SPDX-License-Identifier: GPL-3.0-or-later */

mod handlers;
mod options;
mod multiprocess;
mod simplelog;

use anyhow::{Result, anyhow};
use log::debug;
use std::env;
use std::path::Path;
use std::process::Command;
use std::os::unix::process::CommandExt;

fn maybe_reexec_full() -> Result<()> {
    // If we are called 'add-determinism.nopython', if
    // 'add-determinism' exists, reexec into it.
    let mut args = env::args();

    let arg0 = args.next().unwrap();
    let path = Path::new(&arg0);
    if path.extension().is_some_and(|x| x == "nopython") {
        let path2 = path.with_extension("");
        let args: Vec<_> = args.collect();
        if path2.exists() {
            // Can't use logging here, because it hasn't been initialized yet.
            println!("Reexecuting as {} {}", path2.display(), args.join(" "));
            let err = Command::new(path2).args(args).exec();
            return Err(err.into());
        }
    }

    Ok(()) // no reexec necessary
}

fn brp_check(config: &options::Config) -> Result<()> {
    // env::current_exe() does readlink("/proc/self/exe"), which returns
    // the target binary, so we cannot use that.
    let arg0 = env::args().next().unwrap();

    debug!("Running as {}… (brp={})", arg0, if config.brp { "true" } else { "false" });

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
    maybe_reexec_full()?;

    let config = match options::Config::make()? {
        None => { return Ok(()); },
        Some(some) => some
    };

    brp_check(&config)?;

    if let Some(socket) = config.socket {
        debug!("Running as worker on socket {}", socket);
        multiprocess::do_worker_work(config)

    } else if let Some(jobs) = config.jobs {
        debug!("Running as controller with {} workers", jobs);
        multiprocess::Controller::do_work(config)

    } else {
        // We're not the controller
        handlers::do_normal_work(config)
    }
}
