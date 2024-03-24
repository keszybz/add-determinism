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
pub struct Options {
    /// Paths to operate on
    #[arg(required=true)]
    pub args: Vec<PathBuf>,

    /// Turn on debugging output
    #[arg(short, long)]
    pub verbose: bool,

    /// Store source date epoch here convenience
    #[arg(long, hide=true)]
    pub source_date_epoch: Option<i64>,
}

impl Options {
    pub fn make() -> Result<Options> {
        let mut options = Options::parse();

        let log_level = if options.verbose { LevelFilter::Debug } else { LevelFilter::Info };
        simplelog::init_with_level(log_level)?;

        if options.source_date_epoch.is_none() {
            options.source_date_epoch = match env::var("SOURCE_DATE_EPOCH") {
                Ok(val) => Some(val.parse::<i64>()?),
                Err(_) => None,
            };
        }

        match options.source_date_epoch {
            None => debug!("SOURCE_DATE_EPOCH timestamp: {}", "(unset)"),
            Some(v) => {
                debug!("SOURCE_DATE_EPOCH timestamp: {} ({})",
                       v,
                       Utc.timestamp_opt(v, 0).unwrap());
            },
        }

        Ok(options)
    }
}
