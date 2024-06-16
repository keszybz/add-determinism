/* SPDX-License-Identifier: GPL-3.0-or-later */

use anyhow::{Result, Context, anyhow};
use log::debug;
use std::io::{BufWriter, Read, Seek, Write};
use std::path::Path;
use std::rc::Rc;

use crate::handlers::InputOutputHelper;
use crate::options;

const MAGIC: &[u8] = b"!<arch>\n";

const FILE_HEADER_LENGTH: usize = 60;
const FILE_HEADER_MAGIC: &[u8] = &[0o140, 0o012];

pub struct Ar {
    config: Rc<options::Config>,
}

impl Ar {
    pub fn boxed(config: &Rc<options::Config>) -> Box<dyn super::Processor> {
        Box::new(Self { config: config.clone() })
    }
}

// Like `read_exact`, but EOF is not an error.
fn read_exact_or_zero(mut r: impl Read, buf: &mut [u8]) -> Result<bool> {
    // End of stream is OK, we return an empty buffer
    let n = r.read(buf)?;
    if n == 0 {
        return Ok(false);
    }
    r.read_exact(&mut buf[n..])?;
    Ok(true)
}

impl super::Processor for Ar {
    fn name(&self) -> &str {
        "ar"
    }

    fn filter(&self, path: &Path) -> Result<bool> {
        Ok(path.extension().is_some_and(|x| x == "a"))
    }

    fn process(&self, input_path: &Path) -> Result<bool> {
        let mut have_mod = false;
        let (mut io, mut input) = InputOutputHelper::open(input_path)?;

        let mut buf = [0; MAGIC.len()];
        input.read_exact(&mut buf)?;
        if buf != MAGIC {
            return Err(anyhow!("{}: wrong magic ({:?})", io.input_path.display(), buf));
        }

        io.open_output()?;
        let mut output = BufWriter::new(io.output.as_mut().unwrap());

        output.write_all(&buf)?;

        let ipath = io.input_path.display();
        loop {
            let pos = input.stream_position()?;
            let mut buf = [0; FILE_HEADER_LENGTH];

            debug!("{ipath}: reading file header at offset {pos}");

            if !read_exact_or_zero(&mut input, &mut buf)
                .with_context(|| anyhow!("{}: short read at offset {}",
                                 io.input_path.display(), pos))? {
                break
            }

            // https://en.wikipedia.org/wiki/Ar_(Unix)
            // from   to     Name                      Format
            // 0      15     File name                 ASCII
            // 16     27     File modification date    Decimal
            // 28     33     Owner ID                  Decimal
            // 34     39     Group ID                  Decimal
            // 40     47     File mode                 Octal
            // 48     57     File size in bytes        Decimal
            // 58     59     File magic                \140\012

            if &buf[58..] != FILE_HEADER_MAGIC {
                return Err(anyhow!("{}: wrong magic in file header at offset {}: {:?} != {:?}",
                                   io.input_path.display(), pos, &buf[58..], FILE_HEADER_MAGIC));
            }

            let name = std::str::from_utf8(&buf[0..16])?.trim_end_matches(' ');

            let size = std::str::from_utf8(&buf[48..58])?.trim_end_matches(' ');
            let size = size.parse::<u32>()?;

            if name == "//" {
                // System V/GNU table of long filenames
                debug!("{}: long filename index, size={}", io.input_path.display(), size);
            } else {
                let mtime = std::str::from_utf8(&buf[16..28])?.trim_end_matches(' ');
                let mtime = mtime.parse::<i64>()?;

                let uid = std::str::from_utf8(&buf[28..34])?.trim_end_matches(' ');
                let uid = uid.parse::<u64>()?;

                let gid = std::str::from_utf8(&buf[34..40])?.trim_end_matches(' ');
                let gid = gid.parse::<u64>()?;

                let mode = std::str::from_utf8(&buf[40..48])?.trim_end_matches(' ');
                let mode = mode.parse::<u64>()?;

                debug!("{}: file {:?}, mtime={}, {}:{}, mode={:o}, size={}",
                       io.input_path.display(), name, mtime, uid, gid, mode, size);

                if self.config.source_date_epoch.is_some() && mtime > self.config.source_date_epoch.unwrap() {
                    let source_date_epoch_str = format!("{:<12}", self.config.source_date_epoch.unwrap());

                    buf[16..28].copy_from_slice(source_date_epoch_str.as_bytes());
                    have_mod = true;
                }

                if uid != 0 || gid != 0 {
                    buf[28..34].copy_from_slice(b"0     ");
                    buf[34..40].copy_from_slice(b"0     ");
                    have_mod = true;
                }
            }

            output.write_all(&buf)?;

            let padded_size = size + size % 2;

            let mut buf = vec![0; padded_size.try_into().unwrap()];
            input.read_exact(&mut buf)?;

            output.write_all(&buf)?;
        }

        output.flush()?;
        drop(output);
        io.finalize(have_mod)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_a() {
        let cfg = Rc::new(options::Config::empty(0));
        let h = Ar::boxed(&cfg);

        assert!( h.filter(Path::new("/some/path/libfoobar.a")).unwrap());
        assert!(!h.filter(Path::new("/some/path/libfoobar.aa")).unwrap());
        assert!( h.filter(Path::new("/some/path/libfoobar.a.a")).unwrap());
        assert!(!h.filter(Path::new("/some/path/libfoobara")).unwrap());
        assert!(!h.filter(Path::new("/some/path/a")).unwrap());
        assert!(!h.filter(Path::new("/some/path/a_a")).unwrap());
        assert!(!h.filter(Path::new("/")).unwrap());
    }
}
