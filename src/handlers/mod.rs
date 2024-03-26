/* SPDX-License-Identifier: GPL-3.0-or-later */

pub mod ar;
pub mod jar;
pub mod javadoc;
pub mod pyc;

use anyhow::{Context, Result, anyhow};
use log::{debug, info, warn};
use std::fs;
use std::fs::{File, Metadata};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::os::linux::fs::MetadataExt;
use std::os::unix::fs as unix_fs;

use crate::options;

pub struct Processor {
    /// Return true if the given path looks like it should be processed.
    filter: fn(&Path) -> Result<bool>,

    /// Process file and return true if modifications were made.
    process: fn(&options::Options, &Path) -> Result<bool>,
}

macro_rules! Proc {
    ( $name:ident ) => {
        Processor { filter: $name::filter, process: $name::process }
    }
}

const PROCESSORS: [Processor; 4] = [
    Proc!(ar),
    Proc!(jar),
    Proc!(javadoc),
    Proc!(pyc),
];

pub fn process_file_or_dir(options: &options::Options, input_path: &Path) -> Result<u64> {
    debug!("Looking at path {:?}…", input_path);

    let mut modifications = 0;

    for entry in walkdir::WalkDir::new(input_path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok()) {

            let metadata = entry.metadata()?;
            if !metadata.is_file() {
                continue;
            }

            for processor in PROCESSORS {
                if (processor.filter)(entry.path())? {
                    match (processor.process)(options, entry.path()) {
                        Err(err) => {
                            warn!("Failed to process file: {}", err);
                        },
                        Ok(modified) => {
                            modifications += modified as u64;
                        },
                    }
                }
            }
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
