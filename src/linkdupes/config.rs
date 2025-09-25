/* SPDX-License-Identifier: GPL-3.0-or-later */

use clap::Parser;

use anyhow::Result;
use log::{info, LevelFilter};
use std::ops::Add;
use std::path::PathBuf;
use std::time;

use crate::{setup, simplelog};

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
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Turn on debugging output
    #[arg(short = 'n', long)]
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

    /// Disable selinux context comparisons
    #[arg(long)]
    pub ignore_selinux_context: bool,

    /// Print selinux contexts
    #[arg(long)]
    pub print_selinux_contexts: bool,
}

pub struct Config {
    #[allow(dead_code)]
    pub root: Option<PathBuf>,  // we need this to do selinux lookups

    pub inputs: Vec<PathBuf>,
    pub brp: bool,
    pub fatal_errors: bool,
    pub _verbose: u8,
    pub dry_run: bool,
    pub ignore_mtime: bool,
    pub ignore_mode: bool,
    pub ignore_owner: bool,
    pub source_date_epoch: Option<time::SystemTime>,
    pub print_selinux_contexts: bool,

    #[cfg(feature = "selinux")]
    pub selinux_labels: Option<selinux::label::Labeler<selinux::label::back_end::File>>,
}

impl Config {
    pub fn make() -> Result<Self> {
        let mut root = None;

        let options = Options::parse();

        // log level
        let log_level = match options.verbose {
            0 => LevelFilter::Warn,
            1 => LevelFilter::Info,
            2 => LevelFilter::Debug,
            3.. => LevelFilter::Trace,
        };

        simplelog::init(log_level, false)?;

        // $SOURCE_DATE_EPOCH
        let source_date_epoch = setup::source_date_epoch()?.map(
            |s| time::UNIX_EPOCH.add(time::Duration::new(s as u64, 0))
        );

        // Extra checks in case --brp was specified
        if options.brp {
            root = Some(setup::brp_check(None, &options.inputs)?);
        }

        #[cfg(feature = "selinux")]
        let selinux_labels = if options.ignore_selinux_context {
            None
        } else {
            Some(selinux::label::Labeler::new(&[], false)?)
        };

        // positional args
        if options.inputs.is_empty() && !options.brp {
            info!("No arguments specified, nothing to do. 😎");
        }

        Ok(Self {
            root,
            inputs: options.inputs,
            brp: options.brp,
            fatal_errors: options.brp,
            _verbose: options.verbose,
            dry_run: options.dry_run,
            ignore_mtime: options.ignore_mtime,
            ignore_mode: options.ignore_mode,
            ignore_owner: options.ignore_owner,
            source_date_epoch,
            print_selinux_contexts: options.print_selinux_contexts,

            #[cfg(feature = "selinux")]
            selinux_labels,
        })
    }

    #[allow(dead_code)]
    // FIXME: should this be marked as #[cfg(test)]? But then the tests don't compile.
    pub const fn empty() -> Self {
        Self {
            root: None,
            inputs: vec![],
            brp: false,
            fatal_errors: false,
            _verbose: 0,
            dry_run: false,
            ignore_mtime: false,
            ignore_mode: false,
            ignore_owner: false,
            source_date_epoch: None,
            print_selinux_contexts: false,

            #[cfg(feature = "selinux")]
            selinux_labels: None,
        }
    }
}
