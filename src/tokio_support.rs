// Copyright (C) 2019, 2020 O.S. Systems Sofware LTDA
//
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::{Ownership, Result, READER_BUFFER_SIZE};
use futures::{
    channel::mpsc::{Receiver, Sender},
    io::ErrorKind,
    join,
    stream::FusedStream,
    SinkExt, StreamExt,
};
use std::{
    future::Future,
    io::{Read, Write},
    path::Path,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

struct AsyncReadWrapper {
    rx: Receiver<Vec<u8>>,
}

impl Read for AsyncReadWrapper {
    fn read(&mut self, mut buf: &mut [u8]) -> std::io::Result<usize> {
        if self.rx.is_terminated() {
            return Ok(0);
        }
        assert_eq!(buf.len(), READER_BUFFER_SIZE);
        Ok(match futures::executor::block_on(self.rx.next()) {
            Some(data) => {
                buf.write_all(&data)?;
                data.len()
            }
            None => 0,
        })
    }
}

fn make_async_read_wrapper_and_worker<R>(
    mut read: R,
) -> (AsyncReadWrapper, impl Future<Output = Result<()>>)
where
    R: AsyncRead + Unpin,
{
    let (mut tx, rx) = futures::channel::mpsc::channel(0);
    (AsyncReadWrapper { rx }, async move {
        loop {
            let mut data = vec![0; READER_BUFFER_SIZE];
            let read = read.read(&mut data).await?;
            data.truncate(read);
            if read == 0 || tx.send(data).await.is_err() {
                break;
            }
        }
        Ok(())
    })
}

struct AsyncWriteWrapper {
    tx: Sender<Vec<u8>>,
}

impl Write for AsyncWriteWrapper {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match futures::executor::block_on(self.tx.send(buf.to_owned())) {
            Ok(()) => Ok(buf.len()),
            Err(err) => Err(std::io::Error::new(ErrorKind::Other, err)),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        futures::executor::block_on(self.tx.send(vec![]))
            .map_err(|err| std::io::Error::new(ErrorKind::Other, err))
    }
}

fn make_async_write_wrapper_and_worker<W>(
    mut write: W,
) -> (AsyncWriteWrapper, impl Future<Output = Result<()>>)
where
    W: AsyncWrite + Unpin,
{
    let (tx, mut rx) = futures::channel::mpsc::channel(0);
    (AsyncWriteWrapper { tx }, async move {
        while let Some(v) = rx.next().await {
            if v.is_empty() {
                write.flush().await?;
            } else {
                write.write_all(&v).await?;
            }
        }
        Ok(())
    })
}

async fn wrap_async_read<R, F, T>(read: R, f: F) -> Result<T>
where
    R: AsyncRead + Unpin,
    F: FnOnce(AsyncReadWrapper) -> T + Send + 'static,
    T: Send + 'static,
{
    let (async_read_wrapper, async_read_wrapper_worker) = make_async_read_wrapper_and_worker(read);
    let g = tokio::task::spawn_blocking(move || f(async_read_wrapper));
    let join = join!(g, async_read_wrapper_worker);
    join.1?;
    Ok(join.0?)
}

async fn wrap_async_read_and_write<R, W, F, T>(read: R, write: W, f: F) -> Result<T>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    F: FnOnce(AsyncReadWrapper, AsyncWriteWrapper) -> T + Send + 'static,
    T: Send + 'static,
{
    let (async_read_wrapper, async_read_wrapper_worker) = make_async_read_wrapper_and_worker(read);
    let (async_write_wrapper, async_write_wrapper_worker) =
        make_async_write_wrapper_and_worker(write);
    let g = tokio::task::spawn_blocking(move || f(async_read_wrapper, async_write_wrapper));
    let join = join!(g, async_read_wrapper_worker, async_write_wrapper_worker);
    join.1?;
    join.2?;
    Ok(join.0?)
}

/// Async version of [`list_archive_files`].
///
/// [`list_archive_files`]: ../fn.list_archive_files.html
pub async fn list_archive_files<R>(source: R) -> Result<Vec<String>>
where
    R: AsyncRead + Unpin,
{
    wrap_async_read(source, crate::list_archive_files).await?
}

/// Async version of [`uncompress_data`].
///
/// [`uncompress_data`]: ../fn.uncompress_data.html
pub async fn uncompress_data<R, W>(source: R, target: W) -> Result<usize>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    wrap_async_read_and_write(source, target, |source, target| {
        crate::uncompress_data(source, target)
    })
    .await?
}

/// Async version of [`uncompress_archive`].
///
/// [`uncompress_archive`]: ../fn.uncompress_archive.html
pub async fn uncompress_archive<R>(source: R, dest: &Path, ownership: Ownership) -> Result<()>
where
    R: AsyncRead + Unpin,
{
    let dest = dest.to_owned();
    wrap_async_read(source, move |source| {
        crate::uncompress_archive(source, &dest, ownership)
    })
    .await?
}

/// Async version of [`uncompress_archive_file`].
///
/// [`uncompress_archive_file`]: ../fn.uncompress_archive_file.html
pub async fn uncompress_archive_file<R, W>(source: R, target: W, path: &str) -> Result<usize>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let path = path.to_owned();
    wrap_async_read_and_write(source, target, move |source, target| {
        crate::uncompress_archive_file(source, target, &path)
    })
    .await?
}
