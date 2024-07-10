/* SPDX-License-Identifier: GPL-3.0-or-later */

use anyhow::{anyhow, Result};
use chrono::{TimeZone, Utc};
use clap::Parser;
use log::{debug, info, log, warn, Level, LevelFilter};
use std::env;
use std::os::fd::RawFd;
use std::path::PathBuf;
use std::time;

use crate::handlers;
use crate::simplelog;

#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
struct Options {
    /// Paths to operate on
    #[arg(value_name = "path")]
    pub inputs: Vec<PathBuf>,

    /// Handlers to enable/disable;
    /// use --handler=list to list
    #[arg(long = "handler")]
    pub handlers: Vec<String>,

    /// Adjust behaviour as appropriate for a build root program
    #[arg(long)]
    pub brp: bool,

    /// Turn on debugging output
    #[arg(short, long)]
    pub verbose: bool,

    /// Fail if any modifications would have been made
    #[arg(long)]
    pub check: bool,

    /// Read paths to process from this socket.
    /// When used, an explicit list of handlers must be given.
    #[arg(long,
          hide = true,
          conflicts_with = "inputs",
          conflicts_with = "jobs",
          requires = "handlers")]
    pub job_socket: Option<RawFd>,

    /// Write results of processing to this socket.
    /// When used, an explicit list of handlers must be given.
    #[arg(long,
          hide = true,
          requires = "job_socket")]
    pub result_socket: Option<RawFd>,

    /// Use N worker processes
    #[arg(short = 'j',
          value_name = "N",
          num_args = 0..=1,
          default_missing_value = "0")]
    pub jobs: Option<u32>,

    /// Remove mtime from pyc files
    #[arg(short, long)]
    pub reset_m_time: bool,
}

pub struct Config {
    pub inputs: Vec<PathBuf>,
    pub brp: bool,
    pub verbose: bool,
    pub job_socket: Option<RawFd>,
    pub result_socket: Option<RawFd>,
    pub check: bool,
    pub jobs: Option<u32>,
    pub source_date_epoch: Option<i64>,
    pub handler_names: Vec<&'static str>,
    pub strict_handlers: bool,
    pub reset_m_time: bool,
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

pub fn requested_handlers(filter: &[&str]) -> Result<(Vec<&'static str>, bool)> {
    if filter.iter().any(|x|  x.starts_with('-')) &&
       filter.iter().any(|x| !x.starts_with('-')) {
            return Err(anyhow!("Cannot mix --handler options with '-' and without"));
        }

    if let Some(name) = filter
        .iter()
        .map(|x| x.strip_prefix('-').unwrap_or(x))
        .find(|x| !handlers::handler_names().contains(x))
    {
        return Err(anyhow!("Unknown handler name: {:?}", name));
    }

    let list: Vec<&'static str> = handlers::HANDLERS
        .iter()
        .filter(|(name, _)| filter_by_name(name, filter))
        .map(|(name, _)| *name)
        .collect();

    if list.is_empty() {
        return Err(anyhow!("Requested handler list is empty, will have nothing to do"));
    }

    let strict = !filter.is_empty();
    debug!("Requested handlers: {} (strict={})", list.join(", "), strict);
    Ok((list, strict))
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

        let (handler_names, strict_handlers) = requested_handlers(&handlers)?;

        // positional args

        if options.job_socket.is_none() && options.inputs.is_empty() && !options.brp {
            info!("No arguments specified, nothing to do. 😎");
        }

        // $SOURCE_DATE_EPOCH

        let mut source_date_epoch = match env::var("SOURCE_DATE_EPOCH") {
            Ok(val) => Some(val.parse::<i64>()?),
            Err(_) => None,
        };

        if let Some(v) = source_date_epoch {
            let now = time::SystemTime::now();
            let now_sec = now.duration_since(time::UNIX_EPOCH).unwrap().as_secs();

            let neg = v < 0;
            let pos = v > 0 && v as u64 > now_sec;

            log!(if neg || pos { Level::Warn } else { Level::Debug },
                 "SOURCE_DATE_EPOCH timestamp: {v} ({})",
                 Utc.timestamp_opt(v, 0).unwrap());
            if neg {
                warn!("SOURCE_DATE_EPOCH timestamp is negative, ignoring: {v}");
                source_date_epoch = None;
            } else if pos {
                warn!("SOURCE_DATE_EPOCH timestamp is in the future: {v} > {now_sec}");
            }
        } else {
            debug!("SOURCE_DATE_EPOCH timestamp: {}", "(unset)");
        }

        Ok(Some(Self {
            inputs: options.inputs,
            brp: options.brp,
            verbose: options.verbose,
            job_socket: options.job_socket,
            result_socket: options.result_socket,
            check: options.check,
            jobs: options.jobs,
            source_date_epoch,
            handler_names,
            strict_handlers,
            reset_m_time: options.reset_m_time,
        }))
    }

    #[allow(dead_code)]
    // FIXME: should this be marked as #[cfg(test)]? But then the tests don't compile.
    pub const fn empty(source_date_epoch: i64, check: bool, reset_mtime: bool) -> Self {
        Self {
            inputs: vec![],
            brp: false,
            verbose: false,
            job_socket: None,
            result_socket: None,
            check,
            jobs: None,
            source_date_epoch: Some(source_date_epoch),
            handler_names: vec![],
            strict_handlers: false,
            reset_m_time: reset_mtime,
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
