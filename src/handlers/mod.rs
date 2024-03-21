/* SPDX-License-Identifier: GPL-3.0-or-later */

pub mod ar;

use anyhow::{Context, Result, anyhow};
use log::{debug, info, warn};
use std::fs;
use std::fs::File;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::os::linux::fs::MetadataExt;
use std::os::unix::fs as unix_fs;

use crate::options;

pub struct Processor {
    /// Return true if the given path looks like it should be processed.
    filter: fn(&Path) -> Result<bool>,

    /// Process file and return true if modifications were made.
    handler: fn(&mut FileProcess) -> Result<(PathBuf, File, bool)>,
}

const PROCESSORS: [Processor; 1] = [
    Processor { filter: ar::filter, handler: ar::handler },
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
                    match FileProcess::process(options, entry.path(), &processor.handler) {
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

pub struct FileProcess<'a> {
    options: &'a options::Options,

    input_path: &'a Path,
    input: File,
}

impl<'a> FileProcess<'a> {
    pub fn new(
        options: &'a options::Options,
        input_path: &'a Path,
    ) -> Result<Self> {

        let input = File::open(input_path)
            .with_context(|| format!("Cannot open {:?}", input_path))?;
        // I tried using BufReader, but it returns short reads occasionally.

        Ok(FileProcess {
            options,
            input_path,
            input,
        })
    }

    pub fn process(
        options: &'a options::Options,
        input_path: &'a Path,
        handler: &dyn Fn(&mut FileProcess) -> Result<(PathBuf, File, bool)>,
    ) -> Result<bool> {

        let mut fp = FileProcess::new(options, input_path)?;
        fp.process_and_replace(handler)
    }

    pub fn open_output(&mut self) -> Result<(PathBuf, File)> {

        let input_file_name = match self.input_path.file_name().unwrap().to_str() {
            Some(name) => name,
            None => {
                return Err(anyhow!("Invalid file name {:?}", self.input_path));
            }
        };

        let output_path = self.input_path.with_file_name(format!(".#.{}.tmp", input_file_name));

        let mut openopts = File::options();
        openopts.write(true).create_new(true);

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

        // We give out the open file and expect to get it back later
        Ok((output_path, output))
    }

    pub fn process_and_replace(
        &mut self,
        handler: &dyn Fn(&mut FileProcess) -> Result<(PathBuf, File, bool)>,
    ) -> Result<bool> {

        let input_metadata = self.input.metadata()?;

        // We either get an error or the output is opened.
        // If have_mod is false, we get rid of the file.
        // If have_mod is true, we replace the orignal with the new version.
        let (output_path, output, have_mod) = handler(self)?;

        if have_mod {
            output.set_permissions(input_metadata.permissions())?;
            output.set_modified(input_metadata.modified()?)?;

            match unix_fs::lchown(&output_path, Some(input_metadata.st_uid()), Some(input_metadata.st_gid())) {
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
            fs::rename(&output_path, self.input_path)?;
        } else {
            debug!("{}: discarding modified version", self.input_path.display());
            fs::remove_file(output_path)?;
        }

        Ok(have_mod)
    }
}
