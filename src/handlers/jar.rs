/* SPDX-License-Identifier: GPL-3.0-or-later */

use anyhow::Result;
use log::{debug, info, warn};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::rc::Rc;

use crate::handlers::InputOutputHelper;
use crate::options;

const FILE_HEADER_MAGIC: [u8; 4] = [0x50, 0x4b, 0x03, 0x04];
const CENTRAL_HEADER_FILE_MAGIC: [u8; 4] = [0x50, 0x4b, 0x01, 0x02];

pub struct Jar {
    config: Rc<options::Config>,
}

impl Jar {
    pub fn boxed(config: &Rc<options::Config>) -> Box<dyn super::Processor> {
        Box::new(Self { config: config.clone() })
    }
}

impl super::Processor for Jar {
    fn name(&self) -> &str {
        "jar"
    }

    fn filter(&self, path: &Path) -> Result<bool> {
        Ok(path.extension().is_some_and(|x| x == "jar"))
    }

    fn process(&self, input_path: &Path) -> Result<bool> {
        let mut have_mod = false;
        let (mut io, input) = InputOutputHelper::open(input_path)?;
        let mut input = zip::ZipArchive::new(BufReader::new(input))?;

        io.open_output()?;

        let output = BufWriter::new(io.output.as_ref().unwrap());
        let mut output = zip::ZipWriter::new(output);

        let epoch = self.config.source_date_epoch
            .map(|v| time::OffsetDateTime::from_unix_timestamp(v).unwrap());

        for i in 0..input.len() {
            let file = input.by_index(i)?;
            output.raw_copy_file(file)?;
        }

        output.finish()?;
        drop(output);

        if let Some(epoch) = epoch {
            match zip::DateTime::try_from(epoch) {
                Err(err) => {
                    warn!("Cannot convert epoch {} to zip::DateTime: {}", epoch, err);
                }
                Ok(dos_epoch) => {
                    let ts: [u8; 4] = [
                        (dos_epoch.timepart() & 0xFF).try_into().unwrap(),
                        (dos_epoch.timepart() >> 8).try_into().unwrap(),
                        (dos_epoch.datepart() & 0xFF).try_into().unwrap(),
                        (dos_epoch.datepart() >> 8).try_into().unwrap(),
                    ];

                    debug!("Epoch converted to zip::DateTime: {dos_epoch:?}");
                    debug!("Epoch as buffer: {ts:?}");

                    // Open output again to adjust timestamps
                    let output_path = io.output_path.as_ref().unwrap();
                    let mut output =
                        zip::ZipArchive::new(BufReader::new(File::open(output_path)?))?;

                    let overwrite = io.output.as_mut().unwrap();

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
                                info!("File {}: {}\n  {:?} {:?} {}", i, file.name(), mtime, epoch,
                                      mtime > epoch);

                                if mtime > epoch {
                                    let header_offset = file.header_start();

                                    debug!("{}: {}: seeking to 0x{:08x} (local file header)",
                                           io.output_path.as_ref().unwrap().display(),
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
                                           io.output_path.as_ref().unwrap().display(),
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
            }
        }

        io.finalize(have_mod)
    }
}
