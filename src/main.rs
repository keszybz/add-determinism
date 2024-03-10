use std::env;

use anyhow::Result;
use std::fs::File;
use std::io::{BufWriter,Read,Write,Seek};
use std::path::Path;

pub const MAGIC: &[u8] = b"!<arch>\n";

pub const FILE_HEADER_LENGTH: usize = 60;
pub const FILE_HEADER_MAGIC: &[u8] = &[0o140, 0o12];

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    let source_date_epoch = match env::var("SOURCE_DATE_EPOCH") {
        Ok(val) => Some(val.parse::<u64>()?),
        Err(_) => None,
    };

    let source_date_epoch_str = format!("{:<12}", source_date_epoch.unwrap_or(0));

    let file_path = Path::new(&args[1]);
    dbg!(file_path);

    let mut input = File::open(file_path)?;
    // I tried using BufReader, but it returns short reads occasionally.

    let mut buf = [0; MAGIC.len()];
    input.read_exact(&mut buf)?;
    if buf != MAGIC {
        println!("{:?}: wrong magic ({:?})", file_path, buf);
        return Ok(())
    }

    let input_file_name = match file_path.file_name().unwrap().to_str() {
        Some(name) => name,
        None => {
            println!("{:?}: invalid file name", file_path);
            return Ok(());
        }
    };

    let out_path = file_path.with_file_name(format!(".#.{}.tmp", input_file_name));
    let mut output = BufWriter::new(File::create(out_path)?);

    output.write(&buf)?;
    
    loop {
        let pos = input.stream_position()?;
        let mut buf = [0; FILE_HEADER_LENGTH];

        println!("{:?}: reading file header at offset {}", file_path, pos);

        match input.read(&mut buf)? {
            0 => break,
            FILE_HEADER_LENGTH => {},
            n => {
                println!("{:?}: short read of {} bytes at offset {}", file_path, n, pos);
                return Ok(());
            }
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
            println!("{:?}: wrong magic in file header at offset {}: {:?} != {:?}",
                     file_path, pos, &buf[58..], FILE_HEADER_MAGIC);
            return Ok(())
        }

        let name = std::str::from_utf8(&buf[0..16])?.trim_end_matches(' ');

        let size = std::str::from_utf8(&buf[48..58])?.trim_end_matches(' ');
        let size = u32::from_str_radix(size, 10)?;

        if name == "//" {
            // System V/GNU table of long filenames
            println!("{:?}: long filename index, size={}", file_path, size);
        } else {
            let mtime = std::str::from_utf8(&buf[16..28])?.trim_end_matches(' ');
            let mtime = u64::from_str_radix(mtime, 10)?;

            let uid = std::str::from_utf8(&buf[28..34])?.trim_end_matches(' ');
            let uid = u64::from_str_radix(uid, 10)?;

            let gid = std::str::from_utf8(&buf[34..40])?.trim_end_matches(' ');
            let gid = u64::from_str_radix(gid, 10)?;

            let mode = std::str::from_utf8(&buf[40..48])?.trim_end_matches(' ');
            let mode = u64::from_str_radix(mode, 8)?;

            println!("{:?}: file {:?}, mtime={}, {}:{}, mode={:o}, size={}", file_path, name, mtime, uid, gid, mode, size);

            if mtime > source_date_epoch.unwrap_or(0) {
                buf[16..28].copy_from_slice(source_date_epoch_str.as_bytes());
            }

            if uid != 0 || gid != 0 {
                buf[28..34].copy_from_slice(b"0     ");
                buf[34..40].copy_from_slice(b"0     ");
            }
        }

        output.write(&buf)?;

        let padded_size = size + size % 2;

        let mut buf = vec![0; padded_size.try_into().unwrap()];
        input.read_exact(&mut buf)?;

        output.write(&buf)?;
    }

    output.flush()?;
    
    Ok(())
}
