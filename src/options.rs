/* SPDX-License-Identifier: GPL-3.0-or-later */

use anyhow::Result;
use clap::Parser;
use chrono::{Utc, TimeZone};
use log::{debug, LevelFilter};
use std::env;
use std::path::PathBuf;

use crate::simplelog;

#[derive(Debug)]
#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Options {
    /// Paths to operate on
    #[arg(required=true)]
    pub args: Vec<PathBuf>,

    /// Turn on debugging output
    #[arg(short, long)]
    pub verbose: bool,
}

#[derive(Debug)]
pub struct Config {
    pub args: Vec<PathBuf>,
    pub verbose: bool,
    pub source_date_epoch: Option<i64>,
}

impl Config {
    pub fn make() -> Result<Self> {
        let options = Options::parse();

        let log_level = if options.verbose { LevelFilter::Debug } else { LevelFilter::Info };
        simplelog::init_with_level(log_level)?;

        let source_date_epoch = match env::var("SOURCE_DATE_EPOCH") {
            Ok(val) => Some(val.parse::<i64>()?),
            Err(_) => None,
        };

        match source_date_epoch {
            None => debug!("SOURCE_DATE_EPOCH timestamp: {}", "(unset)"),
            Some(v) => {
                debug!("SOURCE_DATE_EPOCH timestamp: {} ({})",
                       v,
                       Utc.timestamp_opt(v, 0).unwrap());
            },
        }

        Ok(Self {
            args: options.args,
            verbose: options.verbose,
            source_date_epoch,
        })
    }

    #[allow(dead_code)]
    // FIXME: should this be marked as #[cfg(test)]? But then the tests don't compile.
    pub const fn empty(source_date_epoch: i64) -> Self {
        Self {
            args: vec![],
            verbose: false,
            source_date_epoch: Some(source_date_epoch),
        }
    }
}
