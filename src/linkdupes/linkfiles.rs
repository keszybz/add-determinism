/* SPDX-License-Identifier: GPL-3.0-or-later */

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

#[derive(Debug, Default, PartialEq)]
pub struct Stats {
    /// Count of directories that were scanned. This includes both
    /// command-line arguments and subdirectories found in recursive
    /// processing.
    pub directories: u64,

    /// Count of file paths that were scanned. This includes both
    /// command-line arguments and paths found in recursive
    /// processing.
    pub files: u64,

    pub candidate_files: u64,

    /// Count of files that we read or attempted to read
    pub files_read: u64,

    /// Count of files that we linked
    pub files_linked: u64,

    /// Count of files that couldn't be processed
    pub errors: u64,

    /// Summary of sizes of files that were linked
    pub bytes_linked: u64,
}

impl Stats {
    pub fn new() -> Self { Default::default() }

    pub fn summarize(&self) {
        info!(
            "Scanned {} directories and {} files,
    considered {} files, read {} files, linked {} files, {} errors
    sum of sizes of linked files: {} bytes",
            self.directories, self.files,
            self.candidate_files, self.files_read, self.files_linked, self.errors,
            self.bytes_linked);
    }
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

    fn compare(
        &self,
        other: &FileInfo,
        config: &Config,
    ) -> Ordering {
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

        // If the file is empty, we don't need to open it to compare.
        if ms.len() == 0 {
            debug!("Comparing {} and {} → size=0, {:?}",
                   self.path.display(), other.path.display(), Ordering::Equal);
            return Ordering::Equal;
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
        4096u64 * 2u64.pow(min(previous_chunk_count, 8) as u32)
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
    stats: &mut Stats,
) -> Result<()> {

    for entry in walkdir::WalkDir::new(input_path)
        .follow_links(false)
        .into_iter() {
            let entry = match entry {
                Err(e) => {
                    stats.errors += 1;
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
                    stats.errors += 1;
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
                stats.directories += 1;
                continue;
            }

            stats.files += 1;

            if !metadata.is_file() {
                debug!("{}: not a file", entry.path().display());
                continue;
            }

            stats.candidate_files += 1;
            files_seen.push(FileInfo::new(entry.path().to_path_buf(), metadata));
        }

    Ok(())
}

fn link_file(a: &FileInfo, b: &FileInfo, config: &Config) -> Result<bool> {
    // TODO: what happens if we have files a↔b, c↔d,
    // and then we link a←c. We should also link a←d.

    if a.metadata.ino() == b.metadata.ino() {
        info!("Already linked: {} and {}", a.path.display(), b.path.display());
        return Ok(false);
    }

    // Check that b hasn't been modified in the meantime, e.g. by
    // us under a different name.
    let md = b.path.symlink_metadata()?;
    if md.ino() != b.metadata.ino() {
        info!("Ignoring changed {}", b.path.display());
        return Ok(false);
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

    Ok(true)
}

fn link_files(
    files: Vec<FileInfo>,
    config: &Config,
    stats: &mut Stats,
) -> Result<()> {
    let mut linkto: Option<usize> = None;

    // index is used as a workaround here. I expected .into_iter() to give me
    // an object that I can put in linkto. But then the compiler says that the
    // reference outlives the scope. No idea how to go from a reference to the
    // actual object.

    // We update the statistics on files here. We're iterating over the files
    // anyway, so we can do that with very little overhead.

    for (n, finfo) in files.iter().enumerate() {
        // info!("File[{}]: {} (linkto: {:?})", n, finfo.path.display(), linkto);

        if let FileState::None = *finfo.file_state.borrow() { } else {
            stats.files_read += 1;
        }

        if linkto.is_some() &&
           FileInfo::compare(&files[linkto.unwrap()], finfo, config) == Ordering::Equal {

            match link_file(&files[linkto.unwrap()], finfo, config) {
                Ok(res) => {
                    if res {
                        stats.files_linked += 1;
                        // TODO: how to correctly count the case when the linked file was already linked
                        stats.bytes_linked += finfo.metadata.len();
                    }
                }
                Err(e) => {
                    if config.fatal_errors {
                        return Err(e.into());
                    } else {
                        stats.errors += 1;
                        warn!("{}: failed to link to {}: {}",
                              files[linkto.unwrap()].path.display(), finfo.path.display(), e);
                    }
                }
            }
        } else if let FileState::Error = *finfo.file_state.borrow() {
            debug!("Skipping over {} with error…", finfo.path.display());

        } else {
            linkto = Some(n);
        }
    }

    Ok(())
}

pub fn process_inputs(config: &Config) -> Result<Stats> {
    let mut files_seen = vec![];
    let mut stats = Stats::new();

    for input_path in &config.inputs {
        process_file_or_dir(&mut files_seen, input_path, &config, &mut stats)?;
    }

    files_seen.sort_by(|a, b| FileInfo::compare(a, b, &config));

    link_files(files_seen, &config, &mut stats)?;

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn compare_metadata() {
        let mut config = Config::empty();

        let mut file1 = tempfile::NamedTempFile::new().unwrap();
        let mut file2 = tempfile::NamedTempFile::new().unwrap();

        file1.write(b"0").unwrap();
        file2.write(b"0").unwrap();

        let ts = file2.as_file().metadata().unwrap().modified().unwrap();
        file1.as_file().set_modified(ts).unwrap();

        let a = FileInfo {
            path: file1.path().to_path_buf(),
            metadata: fs::metadata(file1.path()).unwrap(),
            hashes: vec![1, 2, 3, 4].into(),
            file_state: FileState::Closed.into(),
        };

        let b = FileInfo {
            path: file2.path().to_path_buf(),
            metadata: fs::metadata(file2.path()).unwrap(),
            hashes: vec![1, 2, 3, 4].into(),
            file_state: FileState::Closed.into(),
        };

        assert_eq!(a.compare(&b, &config), Ordering::Equal);

        b.hashes.borrow_mut().push(5);

        assert_eq!(a.compare(&b, &config), Ordering::Less);

        a.hashes.borrow_mut().push(6);

        assert_eq!(a.compare(&b, &config), Ordering::Greater);

        let a_again = FileInfo {
            path: "/a/b/c".into(),
            metadata: a.metadata.clone(),
            hashes: vec![].into(),
            file_state: FileState::None.into(),
        };

        assert_eq!(a.compare(&a_again, &config), Ordering::Equal);

        // Make mtimes smaller
        file2.as_file().set_modified(
            ts + time::Duration::new(-30i64, 123)
        ).unwrap();

        let mut b_again = FileInfo {
            path: file2.path().to_path_buf(),
            metadata: fs::metadata(file2.path()).unwrap(),
            hashes: a.hashes.borrow().clone().into(),
            file_state: FileState::Closed.into(),
        };

        assert_eq!(a.compare(&b_again, &config), Ordering::Greater);

        // Make mtimes larger
        file2.as_file().set_modified(
            ts + time::Duration::new(30i64, 123)
        ).unwrap();

        b_again.metadata = fs::metadata(file2.path()).unwrap();

        assert_eq!(a.compare(&b_again, &config), Ordering::Less);

        // Ignore mtimes
        config.ignore_mtime = true;

        assert_eq!(a.compare(&b_again, &config), Ordering::Equal);

        // Set $SOURCE_DATE_EPOCH
        config.ignore_mtime = false;
        config.source_date_epoch = Some(ts);

        assert_eq!(a.compare(&b_again, &config), Ordering::Equal);
    }

    #[test]
    fn compare_different_fs() {
        let config = Config::empty();

        let a = FileInfo {
            path: "/dev".into(),
            metadata: fs::metadata("/dev").unwrap(),
            hashes: vec![].into(),
            file_state: FileState::Closed.into(),
        };

        let b = FileInfo {
            path: "/proc".into(),
            metadata: fs::metadata("/proc").unwrap(),
            hashes: vec![].into(),
            file_state: FileState::Closed.into(),
        };

        assert_ne!(a.compare(&b, &config), Ordering::Equal);
    }

    #[test]
    fn compare_unreadable() {
        let mut config = Config::empty();
        config.ignore_mtime = true;

        let mut file1 = tempfile::NamedTempFile::new().unwrap();
        let mut file2 = tempfile::NamedTempFile::new().unwrap();

        file1.write(b"0").unwrap();
        file2.write(b"0").unwrap();

        fs::set_permissions(file1.path(), fs::Permissions::from_mode(0u32)).unwrap();
        fs::set_permissions(file2.path(), fs::Permissions::from_mode(0u32)).unwrap();

        let a = FileInfo {
            path: file1.path().to_path_buf(),
            metadata: fs::metadata(file1.path()).unwrap(),
            hashes: vec![].into(),
            file_state: FileState::None.into(),
        };

        let b = FileInfo {
            path: file2.path().to_path_buf(),
            metadata: fs::metadata(file2.path()).unwrap(),
            hashes: vec![].into(),
            file_state: FileState::None.into(),
        };

        let amiroot = fs::metadata("/proc/self/cmdline").unwrap().uid() == 0;
        let expected = if amiroot {
            Ordering::Equal
        } else {
            a.metadata.ino().cmp(&b.metadata.ino())
        };
        assert_eq!(a.compare(&b, &config), expected);
    }

    #[test]
    fn compare_contents() {
        let mut config = Config::empty();
        config.ignore_mtime = true;

        let mut file1 = tempfile::NamedTempFile::new().unwrap();
        let mut file2 = tempfile::NamedTempFile::new().unwrap();

        for (size, chunk_count) in vec![(0, 0), (4, 1), (4092, 1), (4096, 2), (4096*9, 4)] {
            if size > 0 {
                let data = Vec::from_iter(std::iter::repeat_n(66u8, size));
                file1.write(&data).unwrap();
                file1.flush().unwrap();
                file2.write(&data).unwrap();
                file2.flush().unwrap();
            }

            let a = FileInfo {
                path: file1.path().to_path_buf(),
                metadata: fs::metadata(file1.path()).unwrap(),
                hashes: vec![].into(),
                file_state: FileState::None.into(),
            };

            let b = FileInfo {
                path: file2.path().to_path_buf(),
                metadata: fs::metadata(file2.path()).unwrap(),
                hashes: vec![].into(),
                file_state: FileState::None.into(),
            };

            assert_eq!(a.compare(&b, &config), Ordering::Equal);
            assert_eq!(a.hashes.borrow().len(), chunk_count);
            assert_eq!(b.hashes.borrow().len(), chunk_count);
            assert_eq!(*a.hashes.borrow(), *b.hashes.borrow());
            let _exp_state = if size > 0 { FileState::Closed } else { FileState::None };
            assert!(matches!(a.file_state.borrow(), _exp_state));
            assert!(matches!(b.file_state.borrow(), _exp_state));
        }
    }
}
