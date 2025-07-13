/* SPDX-License-Identifier: GPL-3.0-or-later */

use anyhow::{anyhow, bail, Result};
use clap::Parser;
use log::{debug, info, warn, LevelFilter};
use std::cmp::{min, Ordering};
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hasher};
use std::io::{Read, Seek};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::cell::RefCell;
use std::time::SystemTime;
use std::fs;

use add_determinism::setup;
use add_determinism::simplelog;

#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
struct Options {
    /// Paths to operate on
    #[arg(value_name = "path")]
    pub inputs: Vec<PathBuf>,

    /// Adjust behaviour as appropriate for a build root program
    #[arg(long)]
    pub brp: bool,

    /// Turn on debugging output
    #[arg(short, long)]
    pub verbose: bool,
}

#[derive(Debug)]
enum FileState {
    None,
    Open(fs::File),
    Error,
    Closed,
}

#[derive(Debug)]
struct FileInfo {
    path: PathBuf,
    metadata: fs::Metadata,
    hashes: RefCell<Vec<u64>>,
    file_state: RefCell<FileState>,
}

impl FileInfo {
    fn new(path: PathBuf, metadata: fs::Metadata) -> FileInfo {
        FileInfo {
            path,
            metadata,
            hashes: RefCell::new(vec![]),
            file_state: RefCell::new(FileState::None),
        }
    }

    fn compare(&self, other: &FileInfo) -> Ordering {
        // Return LT, EQ, or GT for the comparison of the two files.
        // We may fail to read one or both of the files.
        // In that case, say that they are *not equal*, and the one with
        // the higher inode number is greater.

        // If files have different size, the contents are different by definition.
        let mut ret = self.metadata.len().cmp(&other.metadata.len());
        // debug!("Comparing {} and {} → size={:?}", self.path.display(), other.path.display(), ret);
        if ret != Ordering::Equal {
            return ret;
        }

        // TODO: check fs, compare as different since we can never link

        // If files point at the same inode, the contents are equal by definition.
        ret = self.metadata.ino().cmp(&other.metadata.ino());
        // debug!("Comparing {} and {} → inode={:?}", self.path.display(), other.path.display(), ret);
        if ret == Ordering::Equal {
            return ret;
        }

        for i in 0.. {
            let hash1 = match self.get_hash(i) {
                Err(_) => { return ret; }
                Ok(hash) => hash,
            };

            let hash2 = match other.get_hash(i) {
                Err(_) => { return ret; }
                Ok(hash) => hash,
            };

            let partial = hash1.cmp(&hash2);
            // debug!("Comparing {} and {} → hash{}={:?}",
            //        self.path.display(), other.path.display(), i, partial);
            if partial != Ordering::Equal {
                return partial;
            }

            if hash1.is_none() && hash2.is_none() {
                return Ordering::Equal;
            }
        }

        unreachable!();
    }

    fn hash_chunk_size(previous_chunk_count: usize) -> u64 {
        4096u64 * 2u64.pow(min(previous_chunk_count, 256) as u32)
    }

    fn get_hash(&self, index: usize) -> Result<Option<u64>> {
        if let Some(val) = self.hashes.borrow().get(index) {
            return Ok(Some(*val));
        }

        // We always read the partial hashes one by one, so get_hash()
        // should never jump over an index.
        // debug!("{}: get_hash({}) [have {}]", self.path.display(), index,
        //        self.hashes.borrow().len());
        assert!(index <= self.hashes.borrow().len());

        self.get_next_hash()
    }

    fn get_next_hash(&self) -> Result<Option<u64>> {
        // Calculate the hash for the next range of bytes.
        // If already at the end of the file, return None.

        // try to calculate the next hash
        let mut file_state = self.file_state.borrow_mut();

        match *file_state {
            FileState::None => {
                // debug!("Opening {}…", self.path.display());

                // Open file, store the error if encountered.
                match fs::File::open(&self.path) {
                    Ok(f) => {
                        *file_state = FileState::Open(f);
                    }
                    Err(e) => {
                        warn!("{}: open failed: {}", self.path.display(), e);
                        *file_state = FileState::Error;
                        return Err(e.into());
                    }
                }
            }
            FileState::Error => { bail!("{} is unreadable", self.path.display()); }
            FileState::Closed => { return Ok(None); }
            _ => {}
        };

        let mut file = match *file_state {
            FileState::Open(ref f) => { f }
            _ => { panic!() }
        };

        let mut hashes = self.hashes.borrow_mut();
        let chunk_size = Self::hash_chunk_size(hashes.len());

        let mut buffer = Vec::new();

        // debug!("{}: reading {} bytes at offset {}",
        //        self.path.display(), chunk_size, file.stream_position().unwrap());
        let count = match file.take(chunk_size).read_to_end(&mut buffer) {
            Ok(count) => count,
            Err(e) => {
                warn!("{}: read failed: {}", self.path.display(), e);
                *file_state = FileState::Error;
                return Err(e.into());
            }
        };

        if (count as u64) < chunk_size {
            *file_state = FileState::Closed;
        }

        if count == 0 {
            return Ok(None);
        }

        let mut hasher = DefaultHasher::new();
        hasher.write(&buffer[..count]);
        let hash = hasher.finish();
        hashes.push(hash);
        Ok(Some(hash))
    }
}

pub fn process_file_or_dir(
    files_seen: &mut Vec<FileInfo>,
    input_path: &Path,
) -> Result<()> {

    for entry in walkdir::WalkDir::new(input_path)
        .follow_links(false)
        .into_iter() {
            let entry = match entry {
                Err(e) => {
                    warn!("Failed to process: {e}");
                    continue;
                }
                Ok(entry) => entry
            };

            // debug!("Looking at {}…", entry.path().display());

            let metadata = entry.metadata()?;
            if metadata.is_dir() {
                continue;
            }

            if !metadata.is_file() {
                debug!("{}: not a file", entry.path().display());
                continue;
            }

            files_seen.push(FileInfo::new(entry.path().to_path_buf(), metadata));
        }

    Ok(())
}

fn link_files(files: Vec<FileInfo>) -> Result<()> {
    let mut linkto: Option<usize> = None;

    for (n, finfo) in files.iter().enumerate() {
        info!("File[{}]: {} (linkto: {:?})", n, finfo.path.display(), linkto);

        if linkto.is_some_and(
            |linkto| { FileInfo::compare(&files[linkto], &finfo) == Ordering::Equal }) {

            if files[linkto.unwrap()].metadata.ino() == finfo.metadata.ino() {
                info!("Already linked: {} and {}", files[linkto.unwrap()].path.display(), finfo.path.display());
            } else {
                info!("Would link {} ← {}", files[linkto.unwrap()].path.display(), finfo.path.display());
            }

        } else if let FileState::Error = *finfo.file_state.borrow() {
            debug!("Skipping over {} with error…", finfo.path.display());

        } else {
            linkto = Some(n);
        }
    }

    Ok(())
}

fn main() -> Result<()> {
    let options = Options::parse();

    let log_level = if options.verbose { LevelFilter::Debug } else { LevelFilter::Info };
    simplelog::init(log_level, false)?;

    let source_date_epoch = setup::source_date_epoch()?;

    rlimit::increase_nofile_limit(u64::MAX)?;

    let mut files_seen = vec![];

    for input_path in &options.inputs {
        process_file_or_dir(&mut files_seen, input_path);
    }

    files_seen.sort_by(FileInfo::compare);

    link_files(files_seen)?;

    Ok(())
}
