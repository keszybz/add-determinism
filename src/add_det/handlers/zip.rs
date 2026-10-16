/* SPDX-License-Identifier: GPL-3.0-or-later */

use anyhow::{bail, Result};
use log::{debug, warn};
use std::io::{Cursor, Read, Write};
use std::path::Path;
use std::sync::Arc;
use time;

use super::{config, InputOutputHelper};

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
    fn boxed(
        config: &Arc<config::Config>,
        extension: &'static str,
    ) -> Box<dyn super::Processor + Send + Sync> {
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

fn to_primitive(t: time::OffsetDateTime) -> time::PrimitiveDateTime {
    time::PrimitiveDateTime::new(t.date(), t.time())
}

fn patch_ts(buf: &mut [u8], header_offset: u64, magic: &[u8; 4], ts_offset: usize, ts: &[u8; 4]) {
    let header = &mut buf[header_offset as usize..];
    assert_eq!(header[..4], *magic);
    header[ts_offset..ts_offset + 4].copy_from_slice(ts);
}

fn format_ts(timestamp: zip::DateTime) -> [u8; 4] {
    let (dp, tp) = (timestamp.datepart(), timestamp.timepart());
    let [tp_lo, tp_hi] = tp.to_le_bytes();
    let [dp_lo, dp_hi] = dp.to_le_bytes();
    [tp_lo, tp_hi, dp_lo, dp_hi]
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
        let dos_epoch = zip::DateTime::try_from(to_primitive(unix_epoch))?;

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
        let (mut io, mut input) = InputOutputHelper::open(input_path, self.config.check, true)?;

        // Work on in-memory buffers: the zip reader seeks around for
        // every entry, which on a file means several syscalls per entry.
        let mut data = Vec::with_capacity(io.input_metadata.len() as usize);
        input.read_to_end(&mut data)?;
        let mut input = zip::ZipArchive::new(Cursor::new(data))?;

        let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));

        for i in 0..input.len() {
            let file = input.by_index(i)?;
            output.raw_copy_file(file)?;
        }

        let mut output = output.finish()?.into_inner();

        if let Some(dos_epoch) = self.dos_epoch {
            let unix_epoch = self.unix_epoch.unwrap();
            let ts = format_ts(dos_epoch);

            // Figure out a bogus timestamp that is not exactly equal
            // to the real one. If unix_epoch is too small, fall back
            // to zero. We need to subtract two seconds, because the
            // timestamp has granularity of two seconds.
            let unix_epoch_m_2 = unix_epoch.saturating_sub(2 * time::Duration::SECOND);
            let dos_epoch_m_2 = zip::DateTime::try_from(to_primitive(unix_epoch_m_2)).unwrap_or(dos_epoch);
            let ts_m_2 = format_ts(dos_epoch_m_2);

            debug!("Epoch converted to zip::DateTime: {dos_epoch:?}");
            debug!("Epoch as buffer: {ts:?}");
            debug!("Epoch for .clj converted to zip::DateTime: {dos_epoch_m_2:?}");
            debug!("Epoch for .clj as buffer: {ts_m_2:?}");

            // Reparse the output to find timestamps to adjust, then patch the buffer.
            let mut patches = vec![];
            let mut archive = zip::ZipArchive::new(Cursor::new(&output[..]))?;

            for i in 0..archive.len() {
                let file = archive.by_index(i)?;

                let Some(last_modified) = file.last_modified() else { continue };
                let name = String::from_utf8_lossy(file.name_raw());
                let is_clj = file.name_raw().ends_with(b".clj");
                match time::PrimitiveDateTime::try_from(last_modified).map(|t| t.assume_utc()) {
                    Err(e) => {
                        warn!("{}: component {}: {}",
                              input_path.display(),
                              name,
                              e);
                    }
                    Ok(mtime) => {
                        let threshold = if is_clj && ts_m_2 != ts {
                            &unix_epoch_m_2
                        } else {
                            &unix_epoch
                        };

                        debug!("File {}: {}\n  {:?} {:?} {}", i, name, mtime, self.unix_epoch,
                               if mtime > *threshold { "mtime needs updating" } else { "mtime is fine" });

                        if mtime > *threshold {
                            debug!("{}: {}: local header at 0x{:08x}, central header at 0x{:08x}",
                                   input_path.display(),
                                   name,
                                   file.header_start(),
                                   file.central_header_start());

                            // Use the bogus reduced timestamp for Clojure .clj files
                            let ts_ref = if is_clj { ts_m_2 } else { ts };
                            patches.push((file.header_start(), file.central_header_start(), ts_ref));

                            have_mod = true;
                        }
                    }
                }
            }
            drop(archive);

            for (local_offset, central_offset, ts_ref) in patches {
                // The timestamp is at offset 10 in the local file header
                // and at offset 12 in the central file header.
                patch_ts(&mut output, local_offset, &FILE_HEADER_MAGIC, 10, &ts_ref);
                patch_ts(&mut output, central_offset, &CENTRAL_HEADER_FILE_MAGIC, 12, &ts_ref);
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

                have_mod = output.len() as u64 != io.input_metadata.len();
            }

        if have_mod {
            io.open_output(false)?;
            io.output.as_mut().unwrap().as_file_mut().write_all(&output)?;
        }

        io.finalize(have_mod)
    }
}
