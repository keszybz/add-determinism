mod processors;
mod options;
mod simplelog;

use anyhow::Result;
use log::warn;

fn main() -> Result<()> {
    let options = options::Options::make()?;

    for input_path in &options.args {
        processors::process_file_or_dir(&options, input_path).unwrap_or_else(|err| {
            warn!("Failed to process file: {}", err);
        });
    }

    Ok(())
}
