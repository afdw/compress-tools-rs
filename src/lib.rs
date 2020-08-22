// Copyright (C) 2019, 2020 O.S. Systems Sofware LTDA
//
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The `compress-tools` crate aims to provide a convenient and easy to use set
//! of methods which builds on top of `libarchive` exposing a small set of it’s
//! functionalities.
//!
//! | Platform | Build Status |
//! | -------- | ------------ |
//! | Linux - x86_64 | [![build status](https://github.com/OSSystems/compress-tools-rs/workflows/CI%20-%20Linux%20-%20x86_64/badge.svg)](https://github.com/OSSystems/compress-tools-rs/actions) |
//! | Linux - AArch64 | [![build status](https://github.com/OSSystems/compress-tools-rs/workflows/CI%20-%20Linux%20-%20AArch64/badge.svg)](https://github.com/OSSystems/compress-tools-rs/actions) |
//! | Linux - ARMv7 | [![build status](https://github.com/OSSystems/compress-tools-rs/workflows/CI%20-%20Linux%20-%20ARMv7/badge.svg)](https://github.com/OSSystems/compress-tools-rs/actions) |
//! | macOS - x86_64 | [![build status](https://github.com/OSSystems/compress-tools-rs/workflows/CI%20-%20macOS%20-%20x86_64/badge.svg)](https://github.com/OSSystems/compress-tools-rs/actions) |
//! | Windows - x86_64 | [![build status](https://github.com/OSSystems/compress-tools-rs/workflows/CI%20-%20Windows%20-%20x86_64/badge.svg)](https://github.com/OSSystems/compress-tools-rs/actions) |
//!
//! ---
//!
//! # Dependencies
//!
//! You must have `libarchive` properly installed on your system in order to use
//! this. If building on *nix systems, `pkg-config` is used to locate the
//! `libarchive`; on Windows `vcpkg` will be used to locating the `libarchive`.
//!
//! The minimum supported Rust version is 1.40.
//!
//! # Features
//!
//! This crate is capable of extracting:
//!
//! * compressed files
//! * archive files
//! * single file from an archive
//!
//! For example, to extract an archive file it is as simple as:
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use compress_tools::*;
//! use std::fs::File;
//! use std::path::Path;
//!
//! let mut source = File::open("tree.tar.gz")?;
//! let dest = Path::new("/tmp/dest");
//!
//! uncompress_archive(&mut source, &dest, Ownership::Preserve)?;
//! # Ok(())
//! # }
//! ```

#[cfg(feature = "async")]
pub mod asynchronous;
mod base;
mod error;
mod ffi;

pub use error::{Error, Result};
pub use base::Ownership;
use base::Mode;
use std::{
    io::{Read, Write},
    os::raw::c_void,
    path::Path,
};
use crate::base::{ArchiveReader, READER_BUFFER_SIZE, ArchiveDiskWriter};

struct SyncPipe<'a> {
    buffer: [u8; READER_BUFFER_SIZE],
    reader: &'a mut dyn Read,
}

impl SyncPipe<'_> {
    fn from<R>(reader: &mut R) -> SyncPipe where R: Read {
        SyncPipe {
            reader,
            buffer: [0; READER_BUFFER_SIZE]
        }
    }
}

impl base::Pipe for SyncPipe<'_> {
    fn buffer(&mut self) -> *const c_void {
        self.buffer.as_ptr() as *const c_void
    }

    fn read(&mut self) -> std::result::Result<usize, Box<dyn std::error::Error>> {
        Ok(self.reader.read(&mut self.buffer)?)
    }
}

/// Get all files in a archive using `source` as a reader.
/// # Example
///
/// ```no_run
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use compress_tools::*;
/// use std::fs::File;
///
/// let mut source = File::open("tree.tar")?;
///
/// let file_list = list_archive_files(&mut source)?;
/// # Ok(())
/// # }
/// ```
pub fn list_archive_files<R>(mut source: R) -> Result<Vec<String>>
where
    R: Read,
{
    let pipe = SyncPipe::from(&mut source);
    ArchiveReader::read_all_entries(pipe)
}

/// Uncompress a file using the `source` need as reader and the `target` as a
/// writer.
///
/// # Example
///
/// ```no_run
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use compress_tools::*;
/// use std::fs::File;
///
/// let mut source = File::open("file.txt.gz")?;
/// let mut target = Vec::default();
///
/// uncompress_data(&mut source, &mut target)?;
/// # Ok(())
/// # }
/// ```
///
/// Slices can be used if you know the exact length of the uncompressed data.
/// ```no_run
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use compress_tools::*;
/// use std::fs::File;
///
/// let mut source = File::open("file.txt.gz")?;
/// let mut target = [0 as u8;313];
///
/// uncompress_data(&mut source, &mut target as &mut [u8])?;
/// # Ok(())
/// # }
/// ```
pub fn uncompress_data<R, W>(mut source: R, target: W) -> Result<usize>
where
    R: Read,
    W: Write,
{
    let pipe = SyncPipe::from(&mut source);
    ArchiveReader::open_with(Mode::RawFormat, pipe, |archive| {
        archive.next_entry()?;
        archive.read_into(target)
    })
}

/// Uncompress an archive using `source` as a reader and `dest` as the
/// destination directory.
///
/// # Example
///
/// ```no_run
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use compress_tools::*;
/// use std::fs::File;
/// use std::path::Path;
///
/// let mut source = File::open("tree.tar.gz")?;
/// let dest = Path::new("/tmp/dest");
///
/// uncompress_archive(&mut source, &dest, Ownership::Preserve)?;
/// # Ok(())
/// # }
/// ```
pub fn uncompress_archive<R>(mut source: R, dest: &Path, ownership: Ownership) -> Result<()>
where
    R: Read,
{
    let pipe = SyncPipe::from(&mut source);
    ArchiveReader::open_with(Mode::AllFormat, pipe, |archive| {
        ArchiveDiskWriter::open_with(ownership, |writer| {
            loop {
                let entry = match archive.next_entry() {
                    Ok(e) => e,
                    Err(err) => return match err {
                        Error::Eof => Ok(()),
                        _ => Err(err)
                    }
                };
                entry.set_pathname(dest.join(entry.pathname()?).to_str().unwrap());
                if let Some(hardlink) = entry.hardlink()? {
                    entry.set_hardlink(dest.join(hardlink).to_str().unwrap())
                }
                writer.write_entry(&entry, |entry_writer| archive.read_into(entry_writer).map(|_|{}))?;
            }
        })
    })
}

/// Uncompress a specific file from an archive. The `source` is used as a
/// reader, the `target` as a writer and the `path` is the relative path for
/// the file to be extracted from the archive.
///
/// # Example
///
/// ```no_run
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use compress_tools::*;
/// use std::fs::File;
///
/// let mut source = File::open("tree.tar.gz")?;
/// let mut target = Vec::default();
///
/// uncompress_archive_file(&mut source, &mut target, "file/path")?;
/// # Ok(())
/// # }
/// ```
pub fn uncompress_archive_file<R, W>(mut source: R, target: W, path: &str) -> Result<usize>
where
    R: Read,
    W: Write,
{
    let pipe = SyncPipe::from(&mut source);
    ArchiveReader::open_with(Mode::AllFormat, pipe, |archive| {
        archive.find_entry(path)?;
        archive.read_into(target)
    })
}