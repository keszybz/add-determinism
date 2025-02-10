/* SPDX-License-Identifier: GPL-3.0-or-later */

use anyhow::{bail, Result};
use log::{debug, info};
use std::io;
use std::io::{BufWriter, Read, Write};
use std::path::Path;
use std::rc::Rc;

use crate::handlers::InputOutputHelper;
use crate::config;

const GZIP_MAGIC: &[u8] = &[0x1F, 0x8B];

// Based on https://www.ietf.org/rfc/rfc1952.txt.

pub struct Gzip {
    config: Rc<config::Config>,
    unix_epoch: Option<u32>,
}

impl Gzip {
    pub fn boxed(config: &Rc<config::Config>) -> Box<dyn super::Processor> {
        Box::new(Self {
            config: config.clone(),
            unix_epoch: None,
        })
    }
}

impl super::Processor for Gzip {
    fn name(&self) -> &str {
        "gzip"
    }

    fn initialize(&mut self) -> Result<()> {
        if self.config.source_date_epoch.is_none() {
            bail!("gzip handler requires $SOURCE_DATE_EPOCH to be set");
        }
        self.unix_epoch = Some(
            self.config.source_date_epoch.unwrap().try_into()?
        );

        Ok(())
    }

    fn filter(&self, path: &Path) -> Result<bool> {
        Ok(
            self.config.ignore_extension ||
                path.extension().is_some_and(|x| x == "gz")
        )
    }

    fn process(&self, input_path: &Path) -> Result<super::ProcessResult> {
        let (mut io, mut input) = InputOutputHelper::open(input_path, self.config.check, true)?;

        let mut buf = [0; 10];
        input.read_exact(&mut buf)?;

        if &buf[0..2] != GZIP_MAGIC {
            return Err(super::Error::BadMagic(2, buf[0..2].to_vec(), GZIP_MAGIC).into());
        }

        let mtime = &buf[4..8];
        let mtime = u32::from_le_bytes(mtime.try_into().unwrap());
        debug!("{}: mtime={} $S_D_E={}", input_path.display(), mtime, self.config.source_date_epoch.unwrap());
        if mtime <= self.unix_epoch.unwrap() {
            return io.finalize(false);
        }

        buf[4..8].copy_from_slice(&self.unix_epoch.unwrap().to_le_bytes());
        info!("{}: changing content mtime from {} ({}) to {} ({})",
              input_path.display(),
              mtime,
              chrono::DateTime::from_timestamp(mtime as i64, 0).unwrap(),
              self.unix_epoch.unwrap(),
              chrono::DateTime::from_timestamp(self.unix_epoch.unwrap() as i64, 0).unwrap());

        io.open_output()?;
        let mut output = BufWriter::new(io.output.as_mut().unwrap());

        output.write_all(&buf)?;

        // Copy the rest unchanged
        io::copy(&mut input, &mut output)?;

        output.flush()?;
        drop(output);

        io.finalize(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_html() {
        let cfg = Rc::new(config::Config::empty(1704106800, false));
        let h = Gzip::boxed(&cfg);

        assert!( h.filter(Path::new("/some/path/page.gz")).unwrap());
        assert!( h.filter(Path::new("/some/path/page.html.gz")).unwrap());
        assert!(!h.filter(Path::new("/some/path/page.html.agz")).unwrap());
        assert!(!h.filter(Path::new("/")).unwrap());
    }
}
