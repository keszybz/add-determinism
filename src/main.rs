mod ar;
mod options;
mod simplelog;

use anyhow::Result;
use log::{debug, warn};
use std::path::Path;
use walkdir;

pub struct Processor {
    filter: fn(&walkdir::DirEntry) -> Result<bool>,
    process: fn(&options::Options, &Path) -> Result<()>,
}

const PROCESSORS: [Processor; 1] = [
    Processor { filter: ar::filter, process: ar::process },
];

fn main() -> Result<()> {
    let options = options::Options::make()?;

    for input_path in &options.args {
        process_file_or_dir(&options, input_path).unwrap_or_else(|err| {
            warn!("Failed to process file: {}", err);
        });
    }

    Ok(())
}

fn process_file_or_dir(options: &options::Options, input_path: &Path) -> Result<()> {
    debug!("Looking at path {:?}…", input_path);

    for entry in walkdir::WalkDir::new(input_path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok()) {

            let metadata = entry.metadata()?;
            if !metadata.is_file() {
                continue;
            }

            for processor in PROCESSORS {
                if (processor.filter)(&entry)? {
                    (processor.process)(options, entry.path()).unwrap_or_else(|err| {
                        warn!("Failed to process file: {}", err);
                    });
                }
            }
        }

    Ok(())
}

