/* SPDX-License-Identifier: GPL-3.0-or-later */

pub mod ar;
pub mod jar;
pub mod javadoc;
pub mod pyc;

use anyhow::{Context, Result, anyhow};
use log::{debug, info, warn};
use std::collections::HashMap;
use std::fs;
use std::fs::{File, Metadata};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::os::linux::fs::MetadataExt as _;
use std::os::unix::fs::MetadataExt as _;
use std::os::unix::fs as unix_fs;

use crate::options;

#[derive(Debug)]
#[derive(PartialEq)]
pub struct Processor {
    pub name: &'static str,

    /// Return true if the given path looks like it should be processed.
    pub filter: fn(&Path) -> Result<bool>,

    /// Process file and return true if modifications were made.
    pub process: fn(&options::Config, &Path) -> Result<bool>,
}

macro_rules! Proc {
    ( $name:ident ) => {
        Processor { name: stringify!($name), filter: $name::filter, process: $name::process }
    }
}

pub const PROCESSORS: [Processor; 4] = [
    Proc!(ar),
    Proc!(jar),
    Proc!(javadoc),
    Proc!(pyc),
];

pub fn handler_names() -> Vec<&'static str> {
    PROCESSORS.iter().map(|p| p.name).collect()
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

pub fn active_handlers(filter: &[&str]) -> Vec<&'static Processor> {
    PROCESSORS
        .iter()
        .filter(|p| filter_by_name(p.name, filter))
        .collect()
}

pub fn inodes_seen() -> HashMap<u64, u8> {
    HashMap::new()
}

pub fn process_file_or_dir(
    config: &options::Config,
    inodes_seen: &mut HashMap<u64, u8>,
    input_path: &Path,
) -> Result<u64> {

    let mut modifications = 0;

    for entry in walkdir::WalkDir::new(input_path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok()) {

            debug!("Looking at {}…", entry.path().display());

            let metadata = entry.metadata()?;
            if !metadata.is_file() {
                debug!("{}: not a file", entry.path().display());
                continue;
            }

            let ino = metadata.ino();
            let mut already_seen = *inodes_seen.get(&ino).unwrap_or(&0);

            for (n_processor, processor) in config.handlers.iter().enumerate() {
                // The same inode can be linked under multiple names
                // with different extensions. Thus, we check if the
                // given processor already handled this file.
                if already_seen & (1 << n_processor) > 0 {
                    debug!("{}: already seen by {} handler",
                           entry.path().display(), processor.name);
                    continue;
                }

                let cond = (processor.filter)(entry.path())?;
                debug!("{}: handler {}: {}", entry.path().display(), processor.name, cond);

                if cond {
                    already_seen |= 1 << n_processor;

                    match (processor.process)(config, entry.path()) {
                        Err(err) => {
                            warn!("{}: failed to process: {}", entry.path().display(), err);
                        },
                        Ok(modified) => {
                            modifications += modified as u64;
                        },
                    }
                }
            }

            inodes_seen.insert(ino, already_seen);
        }

    Ok(modifications)
}

pub struct InputOutputHelper<'a> {
    input_path: &'a Path,
    input_metadata: Metadata,

    output_path: Option<PathBuf>,
    output: Option<File>,
}

impl<'a> InputOutputHelper<'a> {
    pub fn open(input_path: &'a Path) -> Result<(Self, File)> {

        let input = File::open(input_path)
            .with_context(|| format!("Cannot open {:?}", input_path))?;
        // I tried using BufReader, but it returns short reads occasionally.

        let input_metadata = input.metadata()?;

        let io = InputOutputHelper {
            input_path,
            input_metadata,
            output_path: None,
            output: None,
        };

        Ok((io, input))
    }

    pub fn open_output(&mut self) -> Result<()> {
        assert!(self.output_path.is_none());
        assert!(self.output.is_none());

        let input_file_name = match self.input_path.file_name().unwrap().to_str() {
            Some(name) => name,
            None => {
                return Err(anyhow!("Invalid file name {:?}", self.input_path));
            }
        };

        let output_path = self.input_path.with_file_name(format!(".#.{}.tmp", input_file_name));

        let mut openopts = File::options();
        openopts.read(true).write(true).create_new(true);

        let output = match openopts.open(&output_path) {
            Ok(some) => some,
            Err(e) => {
                if e.kind() != ErrorKind::AlreadyExists {
                    return Err(anyhow!("{}: cannot open temporary file: {}", output_path.display(), e));
                }

                debug!("{}: stale temporary file found, removing", output_path.display());
                fs::remove_file(&output_path)?;
                openopts.open(&output_path)?
            }
        };
        self.output_path = Some(output_path);
        self.output = Some(output);

        Ok(())
    }

    pub fn finalize(&mut self, have_mod: bool) -> Result<bool> {
        let meta = &self.input_metadata;

        if have_mod {
            let output_path = self.output_path.as_ref().unwrap();

            let mut output = self.output.as_ref();
            let fallback_output;
            if output.is_none() {
                fallback_output = match File::open(output_path) {
                    Ok(some) => Some(some),
                    Err(e) => {
                        if e.kind() == ErrorKind::NotFound {
                            return Ok(false); // no modifications and nothing to do
                        } else {
                            return Err(anyhow!("{}: cannot reopen temporary file: {}", output_path.display(), e));
                        }
                    },
                };
                output = fallback_output.as_ref();
            }
            let output = output.unwrap();

            output.set_permissions(meta.permissions())?;
            output.set_modified(meta.modified()?)?;

            match unix_fs::lchown(output_path, Some(meta.st_uid()), Some(meta.st_gid())) {
                Ok(()) => {},
                Err(e) => {
                    if e.kind() == ErrorKind::PermissionDenied {
                        warn!("{}: cannot change file ownership, ignoring", self.input_path.display());
                    } else {
                        return Err(anyhow!("{}: cannot change file ownership: {}", self.input_path.display(), e));
                    }
                },
            }

            info!("{}: replacing with normalized version", self.input_path.display());
            fs::rename(output_path, self.input_path)?;

        } else if let Some(output_path) = &self.output_path {
            debug!("{}: discarding modified version", self.input_path.display());
            fs::remove_file(output_path)?;
        }

        Ok(have_mod)
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
