use crate::ffi;
use crate::error::{Error, Result, archive_result};
use std::ffi::{CStr, CString};
use std::os::raw::c_void;
use std::io::Write;
#[cfg(feature = "async")]
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
#[cfg(feature = "async")]
use futures::Future;

pub(crate) unsafe fn find_header(archive_reader: *mut ffi::archive, mut entry: *mut ffi::archive_entry, path: &str) -> Result<()> {
    loop {
        match ffi::archive_read_next_header(archive_reader, &mut entry) {
            ffi::ARCHIVE_OK => {
                let file_name = CStr::from_ptr(ffi::archive_entry_pathname(entry)).to_str();
                if file_name.is_err() {
                    return Err(Error::Utf(file_name.err().unwrap()));
                } else if file_name.unwrap() == path {
                    return Ok(())
                }
            }
            ffi::ARCHIVE_EOF => return Err(Error::FileNotFound),
            _ => return Err(Error::from(archive_reader)),
        }
    }
}

pub(crate) enum Mode {
    AllFormat,
    RawFormat,
    WriteDisk,
}

/// Determine the ownership behavior when unpacking the archive.
pub enum Ownership {
    /// Preserve the ownership of the files when uncompressing the archive.
    Preserve,
    /// Ignore the ownership information of the files when uncompressing the
    /// archive.
    Ignore,
}

pub(crate) const READER_BUFFER_SIZE: usize = 1024;

pub(crate) trait Pipe {
    fn buffer(&mut self) -> *const c_void;
    fn read(&mut self) -> std::result::Result<usize, Box<dyn std::error::Error>>;
}

pub(crate) struct ArchiveEntry {
    inner: *mut ffi::archive_entry,
    need_free: bool
}

impl ArchiveEntry {
    pub fn new() -> Self {
        unsafe {
            let entry = ffi::archive_entry_new();
            ArchiveEntry { inner: entry, need_free: true }
        }
    }

    pub fn pathname(&self) -> Result<String> {
        unsafe {
            CStr::from_ptr(ffi::archive_entry_pathname(self.inner)).to_str()
                .map(|s| s.to_owned())
                .map_err(Error::Utf)
        }
    }

    pub fn set_pathname(&self, pathname: &str) {
        unsafe {
            ffi::archive_entry_set_pathname(self.inner, CString::new(pathname).unwrap().into_raw());
        }
    }

    pub fn hardlink(&self) -> Result<Option<String>> {
        unsafe {
            let ptr = ffi::archive_entry_hardlink(self.inner);
            if ptr.is_null() {
                return Ok(None)
            }
            CStr::from_ptr(ptr).to_str()
                .map(|s| Some(s.to_owned()))
                .map_err(Error::Utf)
        }
    }

    pub fn set_hardlink(&self, hardlink: &str) {
        unsafe {
            ffi::archive_entry_set_hardlink(self.inner, CString::new(hardlink).unwrap().into_raw());
        }
    }
}

impl Drop for ArchiveEntry {
    fn drop(&mut self) {
        if self.need_free {
            unsafe {
                ffi::archive_entry_free(self.inner)
            }
        }
    }
}

pub(crate) struct ArchiveReader {
    inner: *mut ffi::archive,
    opened: bool
}

impl ArchiveReader {
    fn new(mode: Mode) -> Result<ArchiveReader> {
        unsafe {
            let reader = ffi::archive_read_new();
            let obj = ArchiveReader { inner: reader, opened: false };
            match obj.init(mode) {
                Ok(_) => Ok(obj),
                Err(e) => {
                    obj.finalize()?;
                    Err(e)
                }
            }
        }
    }

    unsafe fn init(&self, mode: Mode) -> Result<()> {
        archive_result(
            ffi::archive_read_support_filter_all(self.inner),
            self.inner,
        )?;
        #[allow(unused_variables)] match mode {
            Mode::RawFormat => archive_result(
                ffi::archive_read_support_format_raw(self.inner),
                self.inner,
            )?,
            Mode::AllFormat => archive_result(
                ffi::archive_read_support_format_all(self.inner),
                self.inner,
            )?,
            Mode::WriteDisk => {
                archive_result(
                    ffi::archive_read_support_format_all(self.inner),
                    self.inner,
                )?;
                archive_result(
                    ffi::archive_read_support_format_raw(self.inner),
                    self.inner,
                )?;
            }
        }
        if self.inner.is_null() {
            return Err(Error::NullArchive);
        }
        Ok(())
    }

    fn open<P: Pipe>(&mut self, pipe: P) -> Result<()> {
        unsafe {
            let data = Box::into_raw(Box::new(Box::new(pipe) as Box<dyn Pipe>)) as *mut c_void;
            archive_result(
                ffi::archive_read_open(
                    self.inner,
                    data,
                    None,
                    Some(libarchive_read_callback),
                    None,
                ),
                self.inner,
            )?;
        }
        self.opened = true;
        Ok(())
    }

    fn finalize(self) -> Result<()> {
        unsafe {
            let result = if self.opened {
                archive_result(ffi::archive_read_close(self.inner), self.inner)
            } else {
                Ok(())
            }; // because self.inner must be freed
            archive_result(ffi::archive_read_free(self.inner), self.inner)?;
            result
        }
    }

    pub fn open_with<P: Pipe, R, C: FnOnce(&mut ArchiveReader) -> Result<R>>(mode: Mode, pipe: P, callback: C) -> Result<R> {
        let mut reader = ArchiveReader::new(mode)?;
        match reader.open(pipe) {
            Ok(_) => {
                let result = callback(&mut reader);
                reader.finalize()?;
                result
            },
            Err(e) => {
                reader.finalize()?;
                Err(e)
            }
        }
    }

    #[cfg(feature = "async")]
    pub async fn open_with_async<P: Pipe, R, F: Future<Output = (Result<R>, ArchiveReader)>, C: FnOnce(ArchiveReader) -> F>(mode: Mode, pipe: P, callback: C) -> Result<R> {
        let mut reader = ArchiveReader::new(mode)?;
        match reader.open(pipe) {
            Ok(_) => {
                let result = callback(reader).await;
                result.1.finalize()?;
                result.0
            },
            Err(e) => {
                reader.finalize()?;
                Err(e)
            }
        }
    }

    pub fn read_all_entries<P: Pipe>(pipe: P) -> Result<Vec<String>> {
        ArchiveReader::open_with(Mode::AllFormat, pipe, |archive| {
            let mut entries = Vec::<String>::new();
            loop {
                match archive.next_entry() {
                    Ok(entry) => entries.push(entry.pathname()?),
                    Err(e) => return match e {
                        Error::Eof => Ok(entries),
                        _ => Err(e)
                    }
                }
            }
        })
    }

    pub fn next_entry(&self) -> Result<ArchiveEntry> {
        unsafe {
            let mut entry_ptr = std::ptr::null_mut();
            match ffi::archive_read_next_header(self.inner, &mut entry_ptr) {
                ffi::ARCHIVE_OK => {
                    Ok(ArchiveEntry { inner: entry_ptr, need_free: false })
                }
                ffi::ARCHIVE_EOF => Err(Error::Eof),
                _ => Err(Error::from(self.inner)),
            }
        }
    }

    pub fn find_entry(&self, path: &str) -> Result<ArchiveEntry> {
        loop {
            match self.next_entry() {
                Ok(e) =>{
                    println!("{}", e.pathname()?);
                    if e.pathname()? == path {
                        return Ok(e)
                    }
                },
                Err(err) => return match err {
                    Error::Eof => Err(Error::FileNotFound),
                    _ => Err(err)
                }
            }
        }
    }

    /// Read chunk of data from current entry
    pub fn read_chunk(&self) -> Result<Vec<u8>> {
        unsafe {
            let mut buffer: *const c_void = std::ptr::null();
            let mut offset = 0;
            let mut size = 0;
            match ffi::archive_read_data_block(self.inner, &mut buffer, &mut size, &mut offset) {
                ffi::ARCHIVE_EOF => Err(Error::Eof),
                ffi::ARCHIVE_OK => Ok(Vec::from_raw_parts(buffer as *mut u8, size, size)),
                _ => Err(Error::from(self.inner)),
            }
        }
    }

    /// Read all data from current entry into reader
    pub fn read_into<W>(&self, mut writer: W) -> Result<usize> where W: Write {
        let mut written = 0;
        loop {
            match self.read_chunk() {
                Ok(buf) => {
                    writer.write_all(buf.as_slice())?;
                    written += buf.len();
                },
                Err(err) => return match err {
                    Error::Eof => Ok(written),
                    _ => Err(err)
                }
            }
        }
    }

    /// Read all data from current entry into reader
    #[cfg(feature = "async")]
    pub async fn read_into_async<W>(&self, mut writer: W) -> Result<usize> where W: AsyncWrite + Unpin {
        let mut written = 0;
        loop {
            match self.read_chunk() {
                Ok(buf) => {
                    writer.write_all(buf.as_slice()).await?;
                    written += buf.len();
                },
                Err(err) => return match err {
                    Error::Eof => Ok(written),
                    _ => Err(err)
                }
            }
        }
    }
}

pub(crate) struct EntryWriter {
    inner: *mut ffi::archive
}

impl Write for EntryWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        use std::io::{Error, ErrorKind};
        unsafe {
            archive_result(
                ffi::archive_write_data_block(self.inner, buf.as_ptr() as *const c_void, buf.len(), 0) as i32,
                self.inner
            ).map_err(|e| Error::new(ErrorKind::Other, e))?;
            Ok(buf.len())
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        // already flushed
        Ok(())
    }
}

pub(crate) struct ArchiveDiskWriter {
    inner: *mut ffi::archive
}

impl ArchiveDiskWriter {
    fn new(ownership: Ownership) -> Result<ArchiveDiskWriter> {
        unsafe {
            let writer = ffi::archive_write_disk_new();
            let obj = ArchiveDiskWriter { inner: writer };
            match obj.init(ownership) {
                Ok(_) => Ok(obj),
                Err(e) => {
                    obj.finalize()?;
                    Err(e)
                }
            }
        }
    }

    unsafe fn init(&self, ownership: Ownership) -> Result<()> {
        let mut writer_flags = ffi::ARCHIVE_EXTRACT_TIME
            | ffi::ARCHIVE_EXTRACT_PERM
            | ffi::ARCHIVE_EXTRACT_ACL
            | ffi::ARCHIVE_EXTRACT_FFLAGS
            | ffi::ARCHIVE_EXTRACT_XATTR;

        if let Ownership::Preserve = ownership {
            writer_flags |= ffi::ARCHIVE_EXTRACT_OWNER;
        }

        archive_result(
            ffi::archive_write_disk_set_options(self.inner, writer_flags as i32),
            self.inner,
        )?;
        archive_result(
            ffi::archive_write_disk_set_standard_lookup(self.inner),
            self.inner,
        )?;

        if self.inner.is_null() {
            return Err(Error::NullArchive);
        }

        Ok(())
    }

    fn finalize(self) -> Result<()> {
        unsafe {
            // because self.inner must be freed
            let result = archive_result(ffi::archive_write_close(self.inner), self.inner);
            archive_result(ffi::archive_write_free(self.inner), self.inner)?;
            result
        }
    }

    pub fn open_with<R, C: FnOnce(&mut ArchiveDiskWriter) -> Result<R>>(ownership: Ownership, callback: C) -> Result<R> {
        let mut writer = ArchiveDiskWriter::new(ownership)?;
        let result = callback(&mut writer);
        writer.finalize()?;
        result
    }

    pub fn write_entry<C>(&self, entry: &ArchiveEntry, entry_write_callback: C) -> Result<()> where C: FnOnce(EntryWriter) -> Result<()> {
        unsafe {
            archive_result(ffi::archive_write_header(self.inner, entry.inner), self.inner)?;
            let entry_writer = EntryWriter { inner: self.inner };
            entry_write_callback(entry_writer)?;
            archive_result(ffi::archive_write_finish_entry(self.inner), self.inner)?;
            Ok(())
        }
    }
}

unsafe extern "C" fn libarchive_read_callback(
    archive: *mut ffi::archive,
    client_data: *mut c_void,
    buffer: *mut *const c_void,
) -> ffi::la_ssize_t {
    let pipe: &mut Box<dyn Pipe> = &mut *(client_data as *mut _);
    *buffer = pipe.buffer();
    let result = match pipe.read() {
        Ok(size) => size as ffi::la_ssize_t,
        Err(e) => {
            let description = CString::new(e.to_string()).unwrap();
            ffi::archive_set_error(archive, 1, description.as_ptr());
            -1
        }
    };
    result
}