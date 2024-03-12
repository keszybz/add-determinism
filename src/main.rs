use std::env;

use anyhow::{Context, Result, anyhow};
use std::fs;
use std::io::{BufWriter, ErrorKind, Read, Write, Seek};
use std::os::linux::fs::MetadataExt;
use std::os::unix::fs as unix_fs;
use std::path::Path;

pub const MAGIC: &[u8] = b"!<arch>\n";

pub const FILE_HEADER_LENGTH: usize = 60;
pub const FILE_HEADER_MAGIC: &[u8] = &[0o140, 0o012];

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    let source_date_epoch = match env::var("SOURCE_DATE_EPOCH") {
        Ok(val) => Some(val.parse::<u64>()?),
        Err(_) => None,
    };

    let source_date_epoch_str = format!("{:<12}", source_date_epoch.unwrap_or(0));

    let file_path = Path::new(&args[1]);
    dbg!(file_path);

    let mut input = fs::File::open(file_path)
        .with_context(|| format!("Cannot open {:?}", file_path))?;
    // I tried using BufReader, but it returns short reads occasionally.

    let mut buf = [0; MAGIC.len()];
    input.read_exact(&mut buf)?;
    if buf != MAGIC {
        println!("{}: wrong magic ({:?})", file_path.display(), buf);
        return Ok(())
    }

    let input_file_name = match file_path.file_name().unwrap().to_str() {
        Some(name) => name,
        None => {
            println!("Invalid file name {:?}", file_path);
            return Ok(());
        }
    };

    let input_metadata = input.metadata()?;

    let out_path = file_path.with_file_name(format!(".#.{}.tmp", input_file_name));

    let mut options = fs::File::options();
    options.write(true).create_new(true);

    let output = match options.open(&out_path) {
        Ok(some) => some,
        Err(e) => {
            if e.kind() != ErrorKind::AlreadyExists {
                return Err(anyhow!("{:?}: cannot open temporary file: {}", out_path, e));
            }

            println!("{}: stale temporary file found, removing", out_path.display());
            fs::remove_file(&out_path)?;
            options.open(&out_path)?
        }
    };

    let mut output = BufWriter::new(output);
    let mut have_mod = false;

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
                have_mod = true;
            }

            if uid != 0 || gid != 0 {
                buf[28..34].copy_from_slice(b"0     ");
                buf[34..40].copy_from_slice(b"0     ");
                have_mod = true;
            }
        }

        output.write(&buf)?;

        let padded_size = size + size % 2;

        let mut buf = vec![0; padded_size.try_into().unwrap()];
        input.read_exact(&mut buf)?;

        output.write(&buf)?;
    }

    output.flush()?;

    if have_mod {
        let output = output.into_inner()?;

        output.set_permissions(input_metadata.permissions())?;
        output.set_modified(input_metadata.modified()?)?;

        match unix_fs::lchown(&out_path, Some(input_metadata.st_uid()), Some(input_metadata.st_gid())) {
            Ok(()) => {},
            Err(e) => {
                 if e.kind() == ErrorKind::PermissionDenied {
                     println!("{:?}: cannot change file ownership, ignoring", file_path);
                 } else {
                     return Err(anyhow!("{:?}: cannot change file ownership: {}", file_path, e));
                 }
            },
        }

        println!("{:?}: replacing with normalized version", file_path);
        fs::rename(&out_path, file_path)?;
    } else {
        println!("{:?}: discarding modified version", file_path);
        fs::remove_file(out_path)?;
    }

    Ok(())
}
