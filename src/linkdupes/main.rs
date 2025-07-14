/* SPDX-License-Identifier: GPL-3.0-or-later */

mod config;

use anyhow::{bail, Error, Result};
use log::{debug, info, warn};
use std::cmp::{min, Ordering};
use std::hash::{DefaultHasher, Hasher};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::cell::RefCell;
use std::fs;

use crate::config::Config;

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

    fn compare(&self, other: &FileInfo, config: &Config) -> Ordering {
        // Return LT, EQ, or GT for the comparison of the two files.
        // We may fail to read one or both of the files.
        // In that case, say that they are *not equal*, and the one with
        // the higher inode number is greater.

        let ms = &self.metadata;
        let mo = &other.metadata;

        // If files have different size, the contents are different by definition.
        let mut partial = ms.len().cmp(&mo.len());
        // debug!("Comparing {} and {} → size={:?}", self.path.display(), other.path.display(), partial);
        if partial != Ordering::Equal {
            return partial;
        }

        partial = ms.dev().cmp(&mo.dev());
        if partial != Ordering::Equal {
            return partial;
        }

        // If files point at the same inode, the contents are equal by definition.
        let ino_res = ms.ino().cmp(&mo.ino());
        // debug!("Comparing {} and {} → inode={:?}", self.path.display(), other.path.display(), partial);
        if ino_res == Ordering::Equal {
            return ino_res;
        }

        if !config.ignore_mode {
            partial = ms.permissions().mode().cmp(&mo.permissions().mode());
            if partial != Ordering::Equal {
                debug!("Comparing {} and {} → mode={:?}", self.path.display(), other.path.display(), partial);
                return partial;
            }
        }

        if !config.ignore_owner {
            partial = ms.uid().cmp(&mo.uid());
            if partial != Ordering::Equal {
                debug!("Comparing {} and {} → uid={:?}", self.path.display(), other.path.display(), partial);
                return partial;
            }

            partial = ms.gid().cmp(&mo.gid());
            if partial != Ordering::Equal {
                debug!("Comparing {} and {} → gid={:?}", self.path.display(), other.path.display(), partial);
                return partial;
            }
        }

        if !config.ignore_mtime {
            // mtime is clamped to $SOURCE_DATE_EPOCH, if set.
            let mut t1 = ms.modified().expect("query mtime");
            if let Some(s) = config.source_date_epoch.filter(|s| s < &t1) {
                t1 = s;
            }

            let mut t2 = mo.modified().expect("query mtime");
            if let Some(s) = config.source_date_epoch.filter(|s| s < &t2) {
                t2 = s;
            }

            partial = t1.cmp(&t2);
            if partial != Ordering::Equal {
                debug!("Comparing {} and {} → mtime={:?}", self.path.display(), other.path.display(), partial);
                return partial;
            }
        }

        for i in 0.. {
            let hash1 = match self.get_hash(i) {
                Err(e) => { return FileInfo::file_error(ino_res, e, config); }
                Ok(hash) => hash,
            };

            let hash2 = match other.get_hash(i) {
                Err(e) => { return FileInfo::file_error(ino_res, e, config); }
                Ok(hash) => hash,
            };

            let res = hash1.cmp(&hash2);
            if res != Ordering::Equal {
                debug!("Comparing {} and {} → hash{}={:?}",
                       self.path.display(), other.path.display(), i, partial);
                return res;
            }

            if hash1.is_none() && hash2.is_none() {
                // Both files have been read
                return Ordering::Equal;
            }
        }

        unreachable!();
    }

    fn file_error(partial: Ordering, _err: Error, config: &Config) -> Ordering {
        // Either exit the program or return a partial result,
        // depending on what Config says.
        if config.fatal_errors {
            std::process::exit(1);
        } else {
            partial
        }
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

        let file = match *file_state {
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

fn process_file_or_dir(
    files_seen: &mut Vec<FileInfo>,
    input_path: &Path,
    config: &Config,
) -> Result<()> {

    for entry in walkdir::WalkDir::new(input_path)
        .follow_links(false)
        .into_iter() {
            let entry = match entry {
                Err(e) => {
                    if config.fatal_errors {
                        return Err(e.into());
                    } else {
                        warn!("Failed to process {}: {}", input_path.display(), e);
                        continue;
                    }
                }
                Ok(entry) => entry
            };

            // debug!("Looking at {}…", entry.path().display());

            let metadata = match entry.metadata() {
                Err(e) => {
                    if config.fatal_errors {
                        return Err(e.into());
                    } else {
                        warn!("{}: failed to stat: {}", entry.path().display(), e);
                        continue;
                    }
                }
                Ok(metadata) => metadata
            };

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

fn link_file(a: &FileInfo, b: &FileInfo, config: &Config) -> Result<()> {
    // TODO: what happens if we have files a↔b, c↔d,
    // and then we link a←c. We should also link a←d.

    if a.metadata.ino() == b.metadata.ino() {
        info!("Already linked: {} and {}", a.path.display(), b.path.display());
        return Ok(());
    }

    // Check that b hasn't been modified in the meantime, e.g. by
    // us under a different name.
    let md = b.path.symlink_metadata()?;
    if md.ino() != b.metadata.ino() {
        info!("Ignoring changed {}", b.path.display());
        return Ok(());
    }

    if config.dry_run {
        info!("Would link {} ← {}", a.path.display(), b.path.display());
    } else {
        let tmp = b.path.with_file_name(format!(".#.{}.tmp", b.path.file_name().unwrap().to_str().unwrap()));
        fs::hard_link(&a.path, &tmp)?;
        if let Err(e) = fs::rename(&tmp, &b.path) {
            // clean up temporary file
            if let Err(g) = fs::remove_file(&tmp) {
                warn!("Removal of temporary file {} failed: {}", tmp.display(), g);
            };
            return Err(e.into());
        }

        info!("Linked {} ← {}", a.path.display(), b.path.display());
    }

    Ok(())
}

fn link_files(files: Vec<FileInfo>, config: &Config) -> Result<()> {
    let mut linkto: Option<usize> = None;

    // index is used as a workaround here. I expected .into_iter() to give me
    // an object that I can put in linkto. But then the compiler says that the
    // reference outlives the scope. No idea how to go from a reference to the
    // actual object.

    for (n, finfo) in files.iter().enumerate() {
        // info!("File[{}]: {} (linkto: {:?})", n, finfo.path.display(), linkto);

        if linkto.is_some() &&
           FileInfo::compare(&files[linkto.unwrap()], finfo, config) == Ordering::Equal {

            link_file(&files[linkto.unwrap()], finfo, config)?;

        } else if let FileState::Error = *finfo.file_state.borrow() {
            debug!("Skipping over {} with error…", finfo.path.display());

        } else {
            linkto = Some(n);
        }
    }

    Ok(())
}

fn main() -> Result<()> {
    let config = Config::make()?;

    rlimit::increase_nofile_limit(u64::MAX)?;

    let mut files_seen = vec![];
    for input_path in &config.inputs {
        process_file_or_dir(&mut files_seen, input_path, &config)?;
    }

    files_seen.sort_by(|a, b| FileInfo::compare(a, b, &config));

    link_files(files_seen, &config)?;

    Ok(())
}
