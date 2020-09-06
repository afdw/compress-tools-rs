// Copyright (C) 2019, 2020 O.S. Systems Software LTDA
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
//! The minimum supported Rust version is 1.42.
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

#[cfg(feature = "async_support")]
pub mod async_support;
mod error;
mod ffi;
#[cfg(feature = "futures_support")]
pub mod futures_support;
#[cfg(feature = "tokio_support")]
pub mod tokio_support;

use error::archive_result;
pub use error::{Error, Result};
use std::{
    ffi::{CStr, CString},
    io::{self, Read, Write},
    os::raw::c_void,
    path::Path,
    slice,
};

const READER_BUFFER_SIZE: usize = 1024;

/// Determine the ownership behavior when unpacking the archive.
pub enum Ownership {
    /// Preserve the ownership of the files when uncompressing the archive.
    Preserve,
    /// Ignore the ownership information of the files when uncompressing the
    /// archive.
    Ignore,
}

struct Pipe<'a> {
    reader: &'a mut dyn Read,
    buffer: [u8; READER_BUFFER_SIZE],
}

enum Mode {
    AllFormat,
    RawFormat,
    WriteDisk { ownership: Ownership },
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
    let open_archive = OpenArchive::create(Mode::AllFormat, &mut source)?;
    let archive_entry = ArchiveEntry::new();
    open_archive.run_and_destroy(|open_archive| unsafe {
        let mut file_list = Vec::new();
        loop {
            match ffi::archive_read_next_header2(open_archive.archive_reader, archive_entry.inner) {
                ffi::ARCHIVE_OK => {
                    file_list.push(archive_entry.pathname()?.unwrap().to_owned());
                }
                ffi::ARCHIVE_EOF => return Ok(file_list),
                _ => return Err(Error::from(open_archive.archive_reader)),
            }
        }
    })
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
/// let mut target = [0 as u8; 313];
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
    let open_archive = OpenArchive::create(Mode::RawFormat, &mut source)?;
    let archive_entry = ArchiveEntry::new();
    open_archive.run_and_destroy(|open_archive| unsafe {
        archive_result(
            ffi::archive_read_next_header2(open_archive.archive_reader, archive_entry.inner),
            open_archive.archive_reader,
        )?;
        libarchive_write_data_block(open_archive.archive_reader, target)
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
    let open_archive = OpenArchive::create(Mode::WriteDisk { ownership }, &mut source)?;
    let archive_entry = ArchiveEntry::new();
    open_archive.run_and_destroy(|open_archive| unsafe {
        loop {
            match ffi::archive_read_next_header2(open_archive.archive_reader, archive_entry.inner) {
                ffi::ARCHIVE_EOF => return Ok(()),
                ffi::ARCHIVE_OK => {
                    if let Some(pathname) = archive_entry.pathname()? {
                        archive_entry.set_pathname(Some(dest.join(pathname).to_str().unwrap()));
                    }

                    if let Some(hardlink) = archive_entry.hardlink()? {
                        archive_entry.set_hardlink(Some(dest.join(hardlink).to_str().unwrap()));
                    }

                    ffi::archive_write_header(open_archive.archive_writer, archive_entry.inner);
                    libarchive_copy_data(open_archive.archive_reader, open_archive.archive_writer)?;

                    if ffi::archive_write_finish_entry(open_archive.archive_writer)
                        != ffi::ARCHIVE_OK
                    {
                        return Err(Error::from(open_archive.archive_writer));
                    }
                }
                _ => return Err(Error::from(open_archive.archive_reader)),
            }
        }
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
    let open_archive = OpenArchive::create(Mode::AllFormat, &mut source)?;
    let archive_entry = ArchiveEntry::new();
    open_archive.run_and_destroy(|open_archive| unsafe {
        loop {
            match ffi::archive_read_next_header2(open_archive.archive_reader, archive_entry.inner) {
                ffi::ARCHIVE_OK => {
                    if archive_entry.pathname().unwrap() == Some(path) {
                        break;
                    }
                }
                ffi::ARCHIVE_EOF => {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("path {} doesn't exist inside archive", path),
                    )
                    .into())
                }
                _ => return Err(Error::from(open_archive.archive_reader)),
            }
        }
        libarchive_write_data_block(open_archive.archive_reader, target)
    })
}

struct OpenArchive<'a> {
    archive_reader: *mut ffi::archive,
    archive_writer: *mut ffi::archive,
    pipe: Box<Pipe<'a>>,
}

impl<'a> OpenArchive<'a> {
    fn create<R>(mode: Mode, reader: &'a mut R) -> Result<Self>
    where
        R: Read,
    {
        unsafe {
            let archive_reader = ffi::archive_read_new();
            let archive_writer = ffi::archive_write_disk_new();

            archive_result(
                ffi::archive_read_support_filter_all(archive_reader),
                archive_reader,
            )?;

            match mode {
                Mode::RawFormat => archive_result(
                    ffi::archive_read_support_format_raw(archive_reader),
                    archive_reader,
                )?,
                Mode::AllFormat => archive_result(
                    ffi::archive_read_support_format_all(archive_reader),
                    archive_reader,
                )?,
                Mode::WriteDisk { ownership } => {
                    let mut writer_flags = ffi::ARCHIVE_EXTRACT_TIME
                        | ffi::ARCHIVE_EXTRACT_PERM
                        | ffi::ARCHIVE_EXTRACT_ACL
                        | ffi::ARCHIVE_EXTRACT_FFLAGS
                        | ffi::ARCHIVE_EXTRACT_XATTR;

                    if let Ownership::Preserve = ownership {
                        writer_flags |= ffi::ARCHIVE_EXTRACT_OWNER;
                    };

                    archive_result(
                        ffi::archive_write_disk_set_options(archive_writer, writer_flags as i32),
                        archive_writer,
                    )?;
                    archive_result(
                        ffi::archive_write_disk_set_standard_lookup(archive_writer),
                        archive_writer,
                    )?;
                    archive_result(
                        ffi::archive_read_support_format_all(archive_reader),
                        archive_reader,
                    )?;
                    archive_result(
                        ffi::archive_read_support_format_raw(archive_reader),
                        archive_reader,
                    )?;
                }
            }

            if archive_reader.is_null() || archive_writer.is_null() {
                return Err(Error::NullArchive);
            }

            let mut pipe = Box::new(Pipe {
                reader,
                buffer: [0; READER_BUFFER_SIZE],
            });

            archive_result(
                ffi::archive_read_open(
                    archive_reader,
                    pipe.as_mut() as *mut _ as *mut _,
                    None,
                    Some(libarchive_read_callback),
                    None,
                ),
                archive_reader,
            )?;

            Ok(OpenArchive {
                archive_reader,
                archive_writer,
                pipe,
            })
        }
    }

    fn destroy(self) -> Result<()> {
        unsafe {
            archive_result(
                ffi::archive_read_close(self.archive_reader),
                self.archive_reader,
            )?;
            archive_result(
                ffi::archive_read_free(self.archive_reader),
                self.archive_reader,
            )?;
            archive_result(
                ffi::archive_write_close(self.archive_writer),
                self.archive_writer,
            )?;
            archive_result(
                ffi::archive_write_free(self.archive_writer),
                self.archive_writer,
            )?;
            drop(self.pipe);
            Ok(())
        }
    }

    fn run_and_destroy<T, F>(mut self, f: F) -> Result<T>
    where
        F: FnOnce(&mut Self) -> Result<T>,
    {
        let res = f(&mut self);
        self.destroy()?;
        res
    }
}

struct ArchiveEntry {
    inner: *mut ffi::archive_entry,
}

impl ArchiveEntry {
    fn new() -> Self {
        Self {
            inner: unsafe { ffi::archive_entry_new() },
        }
    }

    fn pathname(&self) -> Result<Option<&str>> {
        unsafe {
            let ptr = ffi::archive_entry_pathname(self.inner);
            if ptr.is_null() {
                Ok(None)
            } else {
                Ok(Some(CStr::from_ptr(ptr).to_str()?))
            }
        }
    }

    fn set_pathname(&self, pathname: Option<&str>) {
        unsafe {
            ffi::archive_entry_set_pathname(
                self.inner,
                if let Some(pathname) = pathname {
                    CString::new(pathname).unwrap().into_raw()
                } else {
                    std::ptr::null()
                },
            );
        }
    }

    fn hardlink(&self) -> Result<Option<&str>> {
        unsafe {
            let ptr = ffi::archive_entry_hardlink(self.inner);
            if ptr.is_null() {
                Ok(None)
            } else {
                Ok(Some(CStr::from_ptr(ptr).to_str()?))
            }
        }
    }

    fn set_hardlink(&self, hardlink: Option<&str>) {
        unsafe {
            ffi::archive_entry_set_hardlink(
                self.inner,
                if let Some(hardlink) = hardlink {
                    CString::new(hardlink).unwrap().into_raw()
                } else {
                    std::ptr::null()
                },
            );
        }
    }
}

impl Drop for ArchiveEntry {
    fn drop(&mut self) {
        unsafe { ffi::archive_entry_free(self.inner) };
    }
}

fn libarchive_copy_data(
    archive_reader: *mut ffi::archive,
    archive_writer: *mut ffi::archive,
) -> Result<()> {
    let mut buffer = std::ptr::null();
    let mut offset = 0;
    let mut size = 0;

    unsafe {
        loop {
            match ffi::archive_read_data_block(archive_reader, &mut buffer, &mut size, &mut offset)
            {
                ffi::ARCHIVE_EOF => return Ok(()),
                ffi::ARCHIVE_OK => {
                    archive_result(
                        ffi::archive_write_data_block(archive_writer, buffer, size, offset) as i32,
                        archive_writer,
                    )?;
                }
                _ => return Err(Error::from(archive_reader)),
            }
        }
    }
}

unsafe fn libarchive_write_data_block<W>(
    archive_reader: *mut ffi::archive,
    mut target: W,
) -> Result<usize>
where
    W: Write,
{
    let mut buffer = std::ptr::null();
    let mut offset = 0;
    let mut size = 0;
    let mut written = 0;

    loop {
        match ffi::archive_read_data_block(archive_reader, &mut buffer, &mut size, &mut offset) {
            ffi::ARCHIVE_EOF => return Ok(written),
            ffi::ARCHIVE_OK => {
                let content = slice::from_raw_parts(buffer as *const u8, size);
                target.write_all(content)?;
                written += size;
            }
            _ => return Err(Error::from(archive_reader)),
        }
    }
}

unsafe extern "C" fn libarchive_read_callback(
    archive: *mut ffi::archive,
    client_data: *mut c_void,
    buffer: *mut *const c_void,
) -> ffi::la_ssize_t {
    let pipe = (client_data as *mut Pipe).as_mut().unwrap();

    *buffer = pipe.buffer.as_ptr() as *const c_void;

    match pipe.reader.read(&mut pipe.buffer) {
        Ok(size) => size as ffi::la_ssize_t,
        Err(e) => {
            let description = CString::new(e.to_string()).unwrap();

            ffi::archive_set_error(archive, e.raw_os_error().unwrap_or(0), description.as_ptr());

            -1
        }
    }
}
