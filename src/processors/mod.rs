/* SPDX-License-Identifier: GPL-3.0-or-later */

pub mod ar;

use anyhow::Result;
use log::{debug, warn};
use std::path::Path;
use crate::options;

pub struct Processor {
    /// Return true if the given path looks like it should be processed.
    filter: fn(&Path) -> Result<bool>,

    /// Process file and return true if modifications were made.
    process: fn(&options::Options, &Path) -> Result<bool>,
}

const PROCESSORS: [Processor; 1] = [
    Processor { filter: ar::filter, process: ar::process },
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
