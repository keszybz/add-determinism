/* SPDX-License-Identifier: GPL-3.0-or-later */

use anyhow::{bail, Result};
use log::{debug, warn};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;
use time;

use crate::handlers::InputOutputHelper;
use crate::config;

const FILE_HEADER_MAGIC: [u8; 4] = [0x50, 0x4b, 0x03, 0x04];
const CENTRAL_HEADER_FILE_MAGIC: [u8; 4] = [0x50, 0x4b, 0x01, 0x02];

pub struct Zip {
    // Share the implementation for .zip and .jar, but define two
    // separate handlers which can be enabled independently.
    extension: &'static str,

    config: Arc<config::Config>,
    unix_epoch: Option<time::OffsetDateTime>,
    dos_epoch: Option<zip::DateTime>,
}

impl Zip {
    fn boxed(config: &Arc<config::Config>, extension: &'static str)
             -> Box<dyn super::Processor + Send + Sync>
    {
        Box::new(Self {
            extension,
            config: config.clone(),
            unix_epoch: None,
            dos_epoch: None,
        })
    }

    pub fn boxed_zip(config: &Arc<config::Config>) -> Box<dyn super::Processor + Send + Sync> {
        Self::boxed(config, "zip")
    }

    pub fn boxed_jar(config: &Arc<config::Config>) -> Box<dyn super::Processor + Send + Sync> {
        Self::boxed(config, "jar")
    }
}

impl super::Processor for Zip {
    fn name(&self) -> &str {
        self.extension
    }

    fn initialize(&mut self) -> Result<()> {
        let unix_epoch = match self.config.source_date_epoch {
            None => bail!("{} handler requires $SOURCE_DATE_EPOCH to be set", self.extension),
            Some(v) => time::OffsetDateTime::from_unix_timestamp(v).unwrap(),
        };
        let dos_epoch = zip::DateTime::try_from(unix_epoch)?;

        self.unix_epoch = Some(unix_epoch);
        self.dos_epoch = Some(dos_epoch);
        Ok(())
    }

    fn filter(&self, path: &Path) -> Result<bool> {
        Ok(self.config.ignore_extension ||
           path.extension().is_some_and(|x| x == self.extension))
    }

    fn process(&self, input_path: &Path) -> Result<super::ProcessResult> {
        let mut have_mod = false;
        let (mut io, input) = InputOutputHelper::open(input_path, self.config.check, true)?;
        let mut input = zip::ZipArchive::new(input)?;

        io.open_output(true)?;

        let output = BufWriter::new(io.output.as_ref().unwrap().as_file());
        let mut output = zip::ZipWriter::new(output);

        for i in 0..input.len() {
            let file = input.by_index(i)?;
            output.raw_copy_file(file)?;
        }

        output.finish()?;
        drop(output);

        if let Some(dos_epoch) = self.dos_epoch {
            let ts: [u8; 4] = [
                (dos_epoch.timepart() & 0xFF).try_into().unwrap(),
                (dos_epoch.timepart() >> 8).try_into().unwrap(),
                (dos_epoch.datepart() & 0xFF).try_into().unwrap(),
                (dos_epoch.datepart() >> 8).try_into().unwrap(),
            ];

            debug!("Epoch converted to zip::DateTime: {dos_epoch:?}");
            debug!("Epoch as buffer: {ts:?}");

            // Open output again to adjust timestamps
            let output_path = io.output.as_ref().unwrap().path().to_path_buf();
            let mut output =
                zip::ZipArchive::new(BufReader::new(File::open(&output_path)?))?;

            let overwrite = io.output.as_mut().unwrap().as_file_mut();

            for i in 0..output.len() {
                let file = output.by_index(i)?;

                match file.last_modified().to_time() {
                    Err(e) => {
                        warn!("{}: component {}: {}",
                              input_path.display(),
                              file.name(),
                              e);
                    }
                    Ok(mtime) => {
                        debug!("File {}: {}\n  {:?} {:?} {}", i, file.name(), mtime, self.unix_epoch,
                               mtime > self.unix_epoch.unwrap());

                        if mtime > self.unix_epoch.unwrap() {
                            let header_offset = file.header_start();

                            debug!("{}: {}: seeking to 0x{:08x} (local file header)",
                                   output_path.display(),
                                   file.name(),
                                   header_offset);

                            overwrite.seek(SeekFrom::Start(header_offset))?;
                            let mut buf = [0; 10];
                            overwrite.read_exact(&mut buf)?;
                            assert_eq!(buf[..4], FILE_HEADER_MAGIC);

                            // We write at offset header_start + 10
                            overwrite.write_all(&ts)?;

                            let header_offset = file.central_header_start();

                            debug!("{}: {}: seeking to 0x{:08x} (central file header)",
                                   output_path.display(),
                                   file.name(),
                                   header_offset);

                            overwrite.seek(SeekFrom::Start(header_offset))?;
                            let mut buf = [0; 12];
                            overwrite.read_exact(&mut buf)?;
                            assert_eq!(buf[..4], CENTRAL_HEADER_FILE_MAGIC);

                            // We write at offset header_start + 12
                            overwrite.write_all(&ts)?;

                            have_mod = true;
                        }
                    }
                }
            }
        }

        if !have_mod &&
            self.unix_epoch.is_some() &&
            io.input_metadata.modified()? > self.unix_epoch.unwrap() {
                // The file's modification timestamp indicates that it
                // was created during the build. This means that it
                // most likely contains uid and gid information that
                // reflects the build environment. This will happen
                // even for files which were copied from build sources
                // and have mtime < $SOURCE_DATE_EPOCH. Our rewriting
                // of the zip file would drop this metadata. Let's
                // check if the rewritten file has different size,
                // which indicates that we dropped some metadata.

                have_mod = io.output.as_mut().unwrap().as_file_mut().metadata()?.len() != io.input_metadata.len();
            }

        io.finalize(have_mod)
    }
}
