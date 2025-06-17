/* SPDX-License-Identifier: GPL-3.0-or-later */

use anyhow::{anyhow, Result};
use clap::Parser;
use log::{debug, info, LevelFilter};
use std::path::PathBuf;
use std::thread;

use crate::handlers;
use crate::setup;
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

    /// Ignore file name extensions, always run handlers.
    #[arg(long)]
    pub ignore_extension: bool,

    /// Print contents of files
    #[arg(short, long,
          conflicts_with = "brp",
          conflicts_with = "check",
          conflicts_with = "jobs")]
    pub print: bool,

    /// Use N worker processes
    #[arg(short = 'j',
          value_name = "N",
          num_args = 0..=1,
          default_missing_value = "0")]
    pub jobs: Option<u32>,
}

pub struct Config {
    pub inputs: Vec<PathBuf>,
    pub brp: bool,
    pub print: bool,
    pub check: bool,
    pub ignore_extension: bool,
    pub jobs: Option<u32>,
    pub source_date_epoch: Option<i64>,
    pub handler_names: Vec<&'static str>,
    pub strict_handlers: bool,
}

fn filter_by_name(name: &str, enabled_by_default: bool, filter: &[&str]) -> bool {
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

    enabled_by_default && negative_filter
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
        .filter(|(name, enabled_by_default, _)| filter_by_name(name, *enabled_by_default, filter))
        .map(|(name, _, _)| *name)
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

        // handlers

        let handlers: Vec<&str> = options.handlers.iter().flat_map(|s| s.split(',')).collect();

        if handlers.contains(&"list") {
            println!("{}", handlers::handler_names().join("\n"));
            return Ok(None);
        }

        // log level

        let log_level = if options.verbose { LevelFilter::Debug } else { LevelFilter::Info };
        // Prefix logs with [pid] if we have multiple workers and -v was used,
        // which means that we expect output from various workers.
        let show_pid = options.verbose && options.jobs.is_some();
        simplelog::init(log_level, show_pid)?;

        let (handler_names, strict_handlers) = requested_handlers(&handlers)?;

        // positional args

        if options.inputs.is_empty() && !options.brp {
            info!("No arguments specified, nothing to do. 😎");
        }

        // $SOURCE_DATE_EPOCH
        let source_date_epoch = setup::source_date_epoch()?;

        let jobs = options.jobs.map(
            |n| if n > 0 { n } else {
                let j = thread::available_parallelism().unwrap().get();
                debug!("Will use {j} workers");
                j as u32
            }
        );

        Ok(Some(Self {
            inputs: options.inputs,
            brp: options.brp,
            print: options.print,
            check: options.check,
            ignore_extension: options.ignore_extension,
            jobs,
            source_date_epoch,
            handler_names,
            strict_handlers,
        }))
    }

    #[allow(dead_code)]
    // FIXME: should this be marked as #[cfg(test)]? But then the tests don't compile.
    pub const fn empty(source_date_epoch: i64, check: bool) -> Self {
        Self {
            inputs: vec![],
            brp: false,
            print: false,
            check,
            ignore_extension: false,
            jobs: None,
            source_date_epoch: Some(source_date_epoch),
            handler_names: vec![],
            strict_handlers: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_by_name() {
        assert_eq!(filter_by_name("x", true, &vec!["x", "y"]), true);
        assert_eq!(filter_by_name("x", true, &vec!["x"]), true);
        assert_eq!(filter_by_name("x", true, &vec![]), true);
        assert_eq!(filter_by_name("x", true, &vec!["-x"]), false);
        assert_eq!(filter_by_name("x", true, &vec!["-y"]), true);

        assert_eq!(filter_by_name("x", false, &vec!["x", "y"]), true);
        assert_eq!(filter_by_name("x", false, &vec!["x"]), true);
        assert_eq!(filter_by_name("x", false, &vec![]), false);
        assert_eq!(filter_by_name("x", false, &vec!["-x"]), false);
        assert_eq!(filter_by_name("x", false, &vec!["-y"]), false);
    }

    #[test]
    fn test_requested_handlers() {
        let (list, strict) = requested_handlers(&vec![]).unwrap();
        assert_eq!(list, vec!["ar", "jar", "javadoc", "gzip", "pyc", "zip"]);
        assert_eq!(strict, false);

        let (list, strict) = requested_handlers(&vec!["ar", "pyc-zero-mtime"]).unwrap();
        assert_eq!(list, vec!["ar", "pyc-zero-mtime"]);
        assert_eq!(strict, true);

        let (list, strict) = requested_handlers(&vec!["-pyc-zero-mtime"]).unwrap();
        assert_eq!(list, vec!["ar", "jar", "javadoc", "gzip", "pyc", "zip"]);
        assert_eq!(strict, true);
    }
}
