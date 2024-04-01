/* SPDX-License-Identifier: GPL-3.0-or-later */

use anyhow::{Result, anyhow};
use clap::Parser;
use chrono::{Utc, TimeZone};
use log::{debug, warn, LevelFilter};
use std::env;
use std::path::PathBuf;

use crate::simplelog;
use crate::handlers;

#[derive(Debug)]
#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Options {
    /// Paths to operate on
    pub args: Vec<PathBuf>,

    /// Handlers to enable;
    /// use --handler=list to list
    #[arg(long = "handler")]
    pub handlers: Vec<String>,

    /// Adjust behaviour as appropriate for a build root program
    #[arg(long)]
    pub brp: bool,

    /// Turn on debugging output
    #[arg(short, long)]
    pub verbose: bool,
}

pub struct Config {
    pub args: Vec<PathBuf>,
    pub brp: bool,
    pub verbose: bool,
    pub source_date_epoch: Option<i64>,
    pub handler_names: Vec<&'static str>,
}

fn filter_by_name(name: &str, filter: &[&str]) -> bool {
    let mut negative_filter = true;

    for f in filter.iter().rev() {
        if let Some(f) = f.strip_prefix('-') {
            if name == f {
                return false;
            }
        } else {
            negative_filter = false;

            if name == *f {
                return true;
            }
        }
    }

    negative_filter
}

pub fn requested_handlers(filter: &[&str]) -> Vec<&'static str> {
    handlers::HANDLERS
        .iter()
        .filter(|(name, _)| filter_by_name(name, filter))
        .map(|(name, _)| *name)
        .collect()
}

impl Config {
    pub fn make() -> Result<Option<Self>> {
        let options = Options::parse();

        // log level

        let log_level = if options.verbose { LevelFilter::Debug } else { LevelFilter::Info };
        simplelog::init_with_level(log_level)?;

        // handlers

        let handlers: Vec<&str> = options.handlers.iter().flat_map(|s| s.split(',')).collect();

        if handlers.contains(&"list") {
            println!("{}", handlers::handler_names().join("\n"));
            return Ok(None);
        }

        if handlers.iter().any(|x| x.starts_with('-')) &&
            handlers.iter().any(|x| !x.starts_with('-')) {
            return Err(anyhow!("Cannot mix --handler options with '-' and without"));
        }

        for name in handlers
            .iter()
            .map(|x| x.strip_prefix('-').unwrap_or(x))
            .filter(|x| !handlers::handler_names().contains(x))
        {
            warn!("Unknown handler name: {:?}", name);
        }

        let handler_names = requested_handlers(&handlers);
        if handler_names.is_empty() {
            return Err(anyhow!("Requested handler list is empty, nothing to do"));
        }
        debug!("Running with handlers: {}", handler_names.join(", "));

        // positional args

        if options.args.is_empty() {
            return Err(anyhow!("Paths to operate on must be specified as positional arguments"));
        }

        // $SOURCE_DATE_EPOCH

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

        // finito

        Ok(Some(Self {
            args: options.args,
            brp: options.brp,
            verbose: options.verbose,
            source_date_epoch,
            handler_names,
        }))
    }

    #[allow(dead_code)]
    // FIXME: should this be marked as #[cfg(test)]? But then the tests don't compile.
    pub const fn empty(source_date_epoch: i64) -> Self {
        Self {
            args: vec![],
            brp: false,
            verbose: false,
            source_date_epoch: Some(source_date_epoch),
            handler_names: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_by_name() {
        assert_eq!(filter_by_name("x", &vec!["x", "y"]), true);
        assert_eq!(filter_by_name("x", &vec!["x"]), true);
        assert_eq!(filter_by_name("x", &vec![]), true);
        assert_eq!(filter_by_name("x", &vec!["-x"]), false);
        assert_eq!(filter_by_name("x", &vec!["-y"]), true);
    }
}
