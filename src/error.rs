// Copyright (C) 2019, 2020 O.S. Systems Software LTDA
//
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::ffi;
use derive_more::{Display, Error, From};
use std::{ffi::CStr, io};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Display, From, Error, Debug)]
#[non_exhaustive]
pub enum Error {
    #[display(fmt = "Extraction error: '{}'", _0)]
    Extraction(#[error(not(source))] String),

    #[display(transparent)]
    Io(io::Error),

    #[display(transparent)]
    Utf(std::str::Utf8Error),

    #[cfg(feature = "tokio_support")]
    #[display(transparent)]
    JoinError(tokio::task::JoinError),

    #[display(fmt = "Error to create the archive struct, is null")]
    NullArchive,

    #[display(fmt = "Unknown error")]
    Unknown,
}

pub(crate) fn archive_result(value: i32, archive: *mut ffi::archive) -> Result<()> {
    if value != ffi::ARCHIVE_OK {
        return Err(Error::from(archive));
    }

    Ok(())
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
impl From<*mut ffi::archive> for Error {
    fn from(input: *mut ffi::archive) -> Self {
        unsafe {
            let error_string = ffi::archive_error_string(input);
            if !error_string.is_null() {
                return Error::Extraction(
                    CStr::from_ptr(error_string).to_string_lossy().to_string(),
                );
            }

            let errno = ffi::archive_errno(input);
            if errno != 0 {
                return io::Error::from_raw_os_error(errno).into();
            }
        }

        Error::Unknown
    }
}
