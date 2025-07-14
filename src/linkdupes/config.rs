/* SPDX-License-Identifier: GPL-3.0-or-later */

use clap::Parser;

use anyhow::Result;
use log::{info, LevelFilter};
use std::path::PathBuf;

use add_determinism::setup;
use add_determinism::simplelog;

#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
struct Options {
    /// Paths to operate on
    #[arg(value_name = "path")]
    pub inputs: Vec<PathBuf>,

    /// Adjust behaviour as appropriate for a build root program
    #[arg(long)]
    pub brp: bool,

    /// Turn on debugging output
    #[arg(short, long)]
    pub verbose: bool,

    /// Turn on debugging output
    #[arg(long)]
    pub dry_run: bool,

    /// Link even if mtimes are different
    #[arg(long)]
    pub ignore_mtime: bool,

    /// Link even if access modes are different
    #[arg(long)]
    pub ignore_mode: bool,
}

pub struct Config {
    pub inputs: Vec<PathBuf>,
    pub fatal_errors: bool,
    pub verbose: bool,
    pub dry_run: bool,
    pub ignore_mtime: bool,
    pub ignore_mode: bool,
    pub source_date_epoch: Option<i64>,
}

impl Config {
    pub fn make() -> Result<Self> {
        let options = Options::parse();

        // log level
        let log_level = if options.verbose { LevelFilter::Debug } else { LevelFilter::Info };
        simplelog::init(log_level, false)?;

        // $SOURCE_DATE_EPOCH
        let source_date_epoch = setup::source_date_epoch()?;

        // Extra checks in case --brp was specified
        if options.brp {
            setup::brp_check(None, &options.inputs)?;
        }

        // positional args
        if options.inputs.is_empty() && !options.brp {
            info!("No arguments specified, nothing to do. 😎");
        }

        Ok(Self {
            inputs: options.inputs,
            fatal_errors: options.brp,
            verbose: options.verbose,
            dry_run: options.dry_run,
            ignore_mtime: options.ignore_mtime,
            ignore_mode: options.ignore_mode,
            source_date_epoch,
        })
    }
}
