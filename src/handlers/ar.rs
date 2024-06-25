/* SPDX-License-Identifier: GPL-3.0-or-later */

use anyhow::Result;
use log::debug;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, Write, ErrorKind};
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
fn read_exact_or_zero(
    input: &mut BufReader<File>,
    buf: &mut [u8],
) -> Result<bool> {

    let pos = input.stream_position()?;

    // End of stream is OK, we return an empty buffer
    let n = input.read(buf)?;
    if n == 0 {
        return Ok(false);
    }
    if let Err(e) = input.read_exact(&mut buf[n..]) {
        if e.kind() == ErrorKind::UnexpectedEof {
            return Err(super::Error::UnexpectedEOF(pos, FILE_HEADER_LENGTH).into());
        } else {
            return Err(e.into());
        }
    }
    Ok(true)
}

impl super::Processor for Ar {
    fn name(&self) -> &str {
        "ar"
    }

    fn filter(&self, path: &Path) -> Result<bool> {
        Ok(path.extension().is_some_and(|x| x == "a"))
    }

    fn process(&self, input_path: &Path) -> Result<super::ProcessResult> {
        let mut have_mod = false;
        let (mut io, mut input) = InputOutputHelper::open(input_path, self.config.check)?;

        let mut buf = [0; MAGIC.len()];
        input.read_exact(&mut buf)?;
        if buf != MAGIC {
            return Err(super::Error::BadMagic(0, buf.to_vec(), MAGIC).into());
        }

        io.open_output()?;
        let mut output = BufWriter::new(io.output.as_mut().unwrap());

        output.write_all(&buf)?;

        loop {
            let pos = input.stream_position()?;
            let mut buf = [0; FILE_HEADER_LENGTH];

            debug!("{}: reading file header at offset {pos}", io.input_path.display());
            if !read_exact_or_zero(&mut input, &mut buf)? {
                break;
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
                return Err(
                    super::Error::BadMagic(pos, buf[58..].to_vec(), FILE_HEADER_MAGIC).into());
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
