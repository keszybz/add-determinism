/* SPDX-License-Identifier: GPL-3.0-or-later */

pub mod ar;
pub mod gzip;
pub mod javadoc;
pub mod pyc;
pub mod zip;

use anyhow::{bail, Context, Result};
use log::{log, debug, info, warn, Level};
use serde::{Serialize, Deserialize};
use std::ascii::escape_default;
use std::collections::HashMap;
use std::fmt::Write;
use std::ffi::OsStr;
use std::fs;
use std::fs::{File, Metadata};
use std::io::{self, Seek};
use std::os::unix::fs as unix_fs;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use thiserror::Error;

#[cfg(target_os = "linux")]
use std::os::linux::fs::MetadataExt as _;
#[cfg(target_os = "macos")]
use std::os::macos::fs::MetadataExt as _;

use crate::config;

#[derive(Error, Debug)]
pub enum Error {
    #[error("unexpected EOF, cannot take {1} bytes at offset 0x{0:x}")]
    UnexpectedEOF(u64, usize),

    #[error("wrong magic at offset {0}\n        (have \"{}\", exp. \"{}\")",
            asciify(.1), asciify(.2))]
    BadMagic(u64, Vec<u8>, &'static [u8]),

    #[error("{0}")]
    Other(String),
}

// based on https://stackoverflow.com/a/52671523
pub fn asciify<B: AsRef<[u8]>>(buf: B) -> String {
    String::from_utf8(
        buf.as_ref()
            .iter()
            .flat_map(|b| escape_default(*b))
            .collect(),
    ).unwrap()
}

#[derive(Copy, Clone, Debug, PartialEq, PartialOrd)]
pub enum ProcessResult {
    Ignored,
    Noop,
    Replaced,
    Rewritten,
    BadFormat,
    Error,
}

impl ProcessResult {
    pub fn convert_and_warn(input_path: &Path, result: Result<ProcessResult>) -> ProcessResult {
        match result {
            Err(err) => {
                warn!("{}: failed to process: {}", input_path.display(), err);

                if err.downcast_ref::<Error>().is_some() {
                    ProcessResult::BadFormat
                } else {
                    ProcessResult::Error
                }
            }
            Ok(res) => res
        }
    }

    pub fn extend_and_warn(&mut self, input_path: &Path, result: Result<ProcessResult>) {
        let converted = ProcessResult::convert_and_warn(input_path, result);

        if (*self == ProcessResult::Replaced && converted == ProcessResult::Rewritten) ||
            (*self == ProcessResult::Rewritten && converted == ProcessResult::Replaced) {
            warn!("{}: different process result, hardlink count modified externally?",
                  input_path.display());
        }

        if *self < converted {
            *self = converted;
        }
    }
}

pub trait Processor {
    fn name(&self) -> &str;

    /// Optionally, do "global" setup of the processor.
    fn initialize(&mut self) -> Result<()> {
        Ok(())
    }

    /// Return true if the given path looks like it should be processed.
    fn filter(&self, path: &Path) -> Result<bool>;

    /// Process file and indicate whether modifications were made.
    fn process(&self, path: &Path) -> Result<ProcessResult>;
}

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    /// Count of directories that were scanned. This includes both
    /// command-line arguments and subdirectories found in recursive
    /// processing.
    pub directories: u64,

    /// Count of file paths that were scanned. This includes both
    /// command-line arguments and paths found in recursive
    /// processing.
    pub files: u64,

    /// Count of inodes we actually processed. We maintain a cache of
    /// processed inode numbers, so a given inode is be processed only
    /// once.
    pub inodes_processed: u64,

    /// Count of inodes modified. Split into inodes that were
    /// automatically replaced and inodes that were rewritten. We
    /// do a rewrite if there are hardlinks to maintain them.
    pub inodes_replaced: u64,
    pub inodes_rewritten: u64,

    /// Files that we couldn't understand.
    /// The case where the file has the right extension, but e.g.
    /// bad magic, do *not* count.
    pub misunderstood: u64,

    /// Various errors other than bad format above.
    pub errors: u64,
}

impl Stats {
    pub fn new() -> Self { Default::default() }

    pub fn add_one(&mut self, result: ProcessResult) {
        match result {
            ProcessResult::Ignored   => { return; }
            ProcessResult::Noop      => {}
            ProcessResult::Replaced  => { self.inodes_replaced += 1;  }
            ProcessResult::Rewritten => { self.inodes_rewritten += 1; }
            ProcessResult::BadFormat => { self.misunderstood += 1;    }
            ProcessResult::Error     => { self.errors += 1;           }
        }

        self.inodes_processed += 1;
    }

    pub fn add(&mut self, other: &Stats) {
        self.directories += other.directories;
        self.files += other.files;
        self.inodes_processed += other.inodes_processed;
        self.inodes_replaced += other.inodes_replaced;
        self.inodes_rewritten += other.inodes_rewritten;
        self.misunderstood += other.misunderstood;
        self.errors += other.errors;
    }

    pub fn summarize(&self) {
        info!("Scanned {} directories and {} files,
               processed {} inodes,
               {} modified ({} replaced + {} rewritten),
               {} unsupported format, {} errors",
              self.directories, self.files,
              self.inodes_processed,
              self.inodes_replaced + self.inodes_rewritten,
              self.inodes_replaced, self.inodes_rewritten,
              self.misunderstood, self.errors);
    }
}

pub type HandlerBoxed = fn(&Rc<config::Config>) -> Box<dyn Processor>;

pub const HANDLERS: &[(&str, bool, HandlerBoxed)] = &[
    ("ar",             true,  ar::Ar::boxed           ),
    ("jar",            true,  zip::Zip::boxed_jar     ),
    ("javadoc",        true,  javadoc::Javadoc::boxed ),
    ("gzip",           true,  gzip::Gzip::boxed       ),
    ("pyc",            true,  pyc::Pyc::boxed         ),
    ("zip",            true,  zip::Zip::boxed_zip     ),
    ("pyc-zero-mtime", false, pyc::PycZeroMtime::boxed),
];

pub fn handler_names() -> Vec<&'static str> {
    HANDLERS.iter()
        .map(|(name, _, _)| *name)
        .collect()
}

pub fn make_handlers(config: &Rc<config::Config>) -> Result<Vec<Box<dyn Processor>>> {
    let mut handlers: Vec<Box<dyn Processor>> = vec![];

    for (name, _, func) in HANDLERS {
        if config.handler_names.contains(name) {
            let mut handler = func(config);
            match handler.initialize() {
                Err(e) => {
                    if config.strict_handlers {
                        bail!("Cannot initialize handler {}: {}", handler.name(), e);
                    }
                    warn!("Handler {} skipped: {}", handler.name(), e);
                }
                Ok(()) => {
                    debug!("Initialized handler {}.", handler.name());
                    handlers.push(handler);
                }
            }
        }
    }

    Ok(handlers)
}

pub fn inodes_seen() -> HashMap<u64, u8> {
    HashMap::new()
}

pub fn do_print(config: &Rc<config::Config>) -> Result<()> {
    let handler = pyc::Pyc::new(config);
    let mut w = String::new();

    for (n, input_path) in config.inputs.iter().enumerate() {
        if n > 0 {
            writeln!(w)?;  // separate outputs by empty line
        }
        handler.pretty_print(&mut w, input_path)?;
    }

    print!("{w}");

    Ok(())
}

pub fn do_normal_work(config: &Rc<config::Config>) -> Result<Stats> {
    let handlers = make_handlers(config)?;
    let mut inodes_seen = inodes_seen();
    let mut total = Stats::new();

    for input_path in &config.inputs {
        let stats = process_file_or_dir(&handlers, &mut inodes_seen, input_path, None);
        total.add(&stats);
    }

    Ok(total)
}

pub type ProcessWrapper<'a> = Option<&'a dyn Fn(u8, &Path) -> Result<()>>;

fn process_file(
    handlers: &[Box<dyn Processor>],
    already_seen: &mut u8,
    input_path: &Path,
    process_wrapper: ProcessWrapper,
) -> Result<ProcessResult> {

    // When processing locally, this says whether modifications have
    // been made. When processing remotely, we will send the result
    // separately after asynchronous processing is finished.
    let mut entry_mod = ProcessResult::Ignored;

    let mut selected_handlers = 0;

    for (n_processor, processor) in handlers.iter().enumerate() {
        // The same inode can be linked under multiple names
        // with different extensions. Thus, we check if the
        // given processor already handled this file.
        if *already_seen & (1 << n_processor) > 0 {
            debug!("{}: already seen by {} handler",
                   input_path.display(), processor.name());
            continue;
        }

        let cond = processor.filter(input_path)?;
        if cond {
            debug!("{}: matched by handler {}", input_path.display(), processor.name());

            selected_handlers |= 1 << n_processor;

            if process_wrapper.is_none() {
                let res = processor.process(input_path);
                entry_mod.extend_and_warn(input_path, res);
            }
        }

        *already_seen |= selected_handlers;
    }

    if selected_handlers > 0 {
        if let Some(func) = process_wrapper {
            assert!(entry_mod == ProcessResult::Ignored);
            func(selected_handlers, input_path)?;
        }
    }

    Ok(entry_mod)
}

fn process_entry(
    handlers: &[Box<dyn Processor>],
    inodes_seen: &mut HashMap<u64, u8>,
    process_wrapper: ProcessWrapper,
    stats: &mut Stats,
    entry: &walkdir::DirEntry,
) -> Result<ProcessResult> {

    debug!("Looking at {}…", entry.path().display());

    let name = unwrap_os_string(entry.file_name())?;
    if name.starts_with(".#.") && name.ends_with(".tmp") {
        // This is our own temporary file. Ignore it.
        return Ok(ProcessResult::Ignored);
    }

    let metadata = entry.metadata()?;
    if metadata.is_dir() {
        stats.directories += 1;
        return Ok(ProcessResult::Ignored);
    }

    stats.files += 1;
    if !metadata.is_file() {
        debug!("{}: not a file", entry.path().display());
        return Ok(ProcessResult::Ignored);
    }

    let inode = metadata.ino();
    let mut already_seen = *inodes_seen.get(&inode).unwrap_or(&0);

    let entry_mod = process_file(
        handlers,
        &mut already_seen,
        entry.path(),
        process_wrapper)?;

    inodes_seen.insert(inode, already_seen); // This is the orig inode
    if entry_mod != ProcessResult::Noop {
        // The path might have been replaced with a new inode.
        let metadata = entry.metadata()?;
        let inode2 = metadata.ino();
        if inode2 != inode {
            // This is the new inode. We use the same set of bits in
            // already_seen, because those handlers have already been
            // applied to the contents of the new inode.
            inodes_seen.insert(inode2, already_seen);
        }
    }

    Ok(entry_mod)
}

pub fn process_file_or_dir(
    handlers: &[Box<dyn Processor>],
    inodes_seen: &mut HashMap<u64, u8>,
    input_path: &Path,
    process_wrapper: ProcessWrapper,
) -> Stats {

    let mut stats = Stats::new();

    for entry in walkdir::WalkDir::new(input_path)
        .follow_links(false)
        .into_iter() {
            let entry = match entry {
                Err(e) => {
                    warn!("Failed to process: {e}");
                    stats.errors += 1;
                    continue;
                }
                Ok(entry) => entry
            };

            let res = process_entry(handlers, inodes_seen, process_wrapper, &mut stats, &entry);
            stats.add_one(ProcessResult::convert_and_warn(entry.path(), res));
        }

    stats
}

pub struct InputOutputHelper<'a> {
    pub input_path: &'a Path,
    pub input_metadata: Metadata,

    // those two are set when .open_output is called
    pub output_path: Option<PathBuf>,
    pub output: Option<File>,

    pub check: bool,
    pub verbose: bool,  // include logging about each modified file
}

impl Drop for InputOutputHelper<'_> {
    fn drop(&mut self) {
        if !self.check {
            if let Some(output_path) = self.output_path.take() {
                debug!("{}: discarding temporary copy", output_path.display());
                if let Err(e) = fs::remove_file(output_path) {
                    if e.kind() != io::ErrorKind::NotFound {
                        warn!("Failed to remove {}: {}", self.input_path.display(), e);
                    }
                }
            }
        }
    }
}

fn unwrap_os_string(filename: &OsStr) -> Result<&str> {
    match filename.to_str() {
        Some(s) => Ok(s),
        None => {
            bail!("Invalid file name {:?}", filename);
        }
    }
}

impl<'a> InputOutputHelper<'a> {
    pub fn open(
        input_path: &'a Path,
        check: bool,
        verbose: bool,
    ) -> Result<(Self, io::BufReader<File>)> {

        let input = File::open(input_path)
            .with_context(|| format!("Cannot open {input_path:?}"))?;

        let input_metadata = input.metadata()?;
        let input = io::BufReader::new(input);

        let io = InputOutputHelper {
            input_path,
            input_metadata,
            output_path: None,
            output: None,
            check,
            verbose,
        };

        Ok((io, input))
    }

    pub fn open_output(&mut self) -> Result<()> {
        assert!(self.output_path.is_none());
        assert!(self.output.is_none());

        let (output, output_path);

        if self.check {
            // TODO: use std::io::Sink here
            output_path = PathBuf::from("/dev/null");
            output = File::options()
                .read(true)
                .write(true)
                .open("/dev/null")?;
        } else {
            let input_file_name = unwrap_os_string(self.input_path.file_name().unwrap())?;
            output_path = self.input_path.with_file_name(format!(".#.{input_file_name}.tmp"));

            let mut openopts = File::options();
            openopts
                .read(true)
                .write(true)
                .create_new(true);

            output = match openopts.open(&output_path) {
                Ok(some) => some,
                Err(e) => {
                    if e.kind() != io::ErrorKind::AlreadyExists {
                        bail!("{}: cannot open temporary file: {}", output_path.display(), e);
                    }

                    info!("{}: stale temporary file found, removing", output_path.display());
                    fs::remove_file(&output_path)?;
                    openopts.open(&output_path)?
                }
            };
        }

        self.output_path = Some(output_path);
        self.output = Some(output);
        Ok(())
    }

    pub fn finalize(&mut self, have_mod: bool) -> Result<ProcessResult> {
        let meta = &self.input_metadata;

        if !have_mod {
            Ok(ProcessResult::Noop)

        } else if self.check {
            // nothing to do, we're using a fake output
            Ok(
                if meta.nlink() == 1 {
                    ProcessResult::Replaced
                } else {
                    ProcessResult::Rewritten
                }
            )

        } else {
            let output_path = self.output_path.as_ref().unwrap();

            let mut output = self.output.as_mut();
            let mut fallback_output;

            if output.is_none() {
                fallback_output = match File::open(output_path) {
                    Ok(some) => Some(some),
                    Err(e) => {
                        if e.kind() == io::ErrorKind::NotFound {
                            return Ok(ProcessResult::Noop); // no modifications and nothing to do
                        } else {
                            bail!("{}: cannot reopen temporary file: {}", output_path.display(), e);
                        }
                    }
                };
                output = fallback_output.as_mut();
            }
            let output = output.unwrap();

            // If the original file has nlinks == 1, we atomically replace it.
            // If it has multiple links, we reopen the original file and rewrite it.
            // This way the inode number is retained and hard links are not broken.
            if meta.nlink() == 1 {
                log!(if self.verbose { Level::Info } else { Level::Debug },
                     "{}: replacing with normalized version", self.input_path.display());

                if !self.check {
                    output.set_permissions(meta.permissions())?;
                    output.set_modified(meta.modified()?)?;

                    if let Err(e) = unix_fs::lchown(output_path, Some(meta.st_uid()), Some(meta.st_gid())) {
                        if e.kind() == io::ErrorKind::PermissionDenied {
                            warn!("{}: cannot change file ownership, ignoring", self.input_path.display());
                        } else {
                            bail!("{}: cannot change file ownership: {}", self.input_path.display(), e);
                        }
                    }

                    fs::rename(output_path, self.input_path)?;
                    self.output_path = None; /* The path is now invalid */
                }

                Ok(ProcessResult::Replaced)

            } else {
                log!(if self.verbose { Level::Info } else { Level::Debug },
                     "{}: rewriting with normalized contents", self.input_path.display());

                if !self.check {
                    output.seek(io::SeekFrom::Start(0))?;

                    let mut input_writer = File::options().write(true).open(self.input_path)?;
                    let len = io::copy(output, &mut input_writer)?;
                    // truncate the file in case it was originally longer
                    input_writer.set_len(len)?;
                    input_writer.set_modified(meta.modified()?)?;
                }

                Ok(ProcessResult::Rewritten)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_asciify() {
        assert_eq!(asciify("asdf"), "asdf");
        assert_eq!(asciify("\"\""), "\\\"\\\"");
        assert_eq!(asciify("\n\t\r"), "\\n\\t\\r");
        assert_eq!(asciify("zębina"), "z\\xc4\\x99bina");
        assert_eq!(asciify([0; 4]), "\\x00\\x00\\x00\\x00");
    }
}
