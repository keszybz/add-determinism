/* SPDX-License-Identifier: GPL-3.0-or-later */

use clap::Parser;

use anyhow::Result;
use log::{info, LevelFilter};
use std::ops::Add;
use std::path::PathBuf;
use std::time;

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

    /// Link even if owner or group are different
    #[arg(long)]
    pub ignore_owner: bool,
}

pub struct Config {
    pub inputs: Vec<PathBuf>,
    pub fatal_errors: bool,
    pub _verbose: bool,
    pub dry_run: bool,
    pub ignore_mtime: bool,
    pub ignore_mode: bool,
    pub ignore_owner: bool,
    pub source_date_epoch: Option<time::SystemTime>,
}

impl Config {
    pub fn make() -> Result<Self> {
        let options = Options::parse();

        // log level
        let log_level = if options.verbose { LevelFilter::Debug } else { LevelFilter::Info };
        simplelog::init(log_level, false)?;

        // $SOURCE_DATE_EPOCH
        let source_date_epoch = setup::source_date_epoch()?.map(
            |s| time::UNIX_EPOCH.add(time::Duration::new(s as u64, 0))
        );

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
            _verbose: options.verbose,
            dry_run: options.dry_run,
            ignore_mtime: options.ignore_mtime,
            ignore_mode: options.ignore_mode,
            ignore_owner: options.ignore_owner,
            source_date_epoch,
        })
    }
}
