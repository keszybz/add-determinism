/* SPDX-License-Identifier: GPL-3.0-or-later */

mod config;
mod handlers;
mod multiprocess;
mod setup;
mod simplelog;

use anyhow::{bail, Result};
use log::debug;
use std::env;
use std::rc::Rc;

fn main() -> Result<()> {
    let config = match config::Config::make()? {
        None => { return Ok(()); }
        Some(some) => some
    };
    let config = Rc::new(config);

    if config.print {
        return handlers::do_print(&config);
    }

    // env::current_exe() does readlink("/proc/self/exe"), which returns
    // the target binary, so we cannot use that.
    let arg0 = env::args().next().unwrap();

    debug!("Running as {arg0}… (brp={})", if config.brp { "true" } else { "false" });

    if config.brp {
        setup::brp_check(None, &config.inputs)?;
    }

    let stats;

    if let Some(socket) = config.job_socket {
        debug!("Running as worker on job socket {socket}");
        return multiprocess::do_worker_work(&config);

    } else if let Some(jobs) = config.jobs {
        debug!("Running as controller with {jobs} workers");
        stats = multiprocess::Controller::do_work(&config)?;

    } else {
        // We're not the controller
        stats = handlers::do_normal_work(&config)?;
    }

    stats.summarize();

    if (config.check || !config.brp) && stats.errors > 0 {
        // Make any errors fatal, except when --brp (without --check) is used.
        // In normal rpms builds, we don't want the build to fail.
        bail!("processing failed")
    } else if config.check && stats.misunderstood > 0 {
        bail!("--check was specified, but some files couldn't be processed")
    } else if config.check && (stats.inodes_replaced > 0 ||
                               stats.inodes_rewritten > 0) {
        bail!("--check was specified, but some files would have been modified")
    }  else {
        Ok(())
    }
}
