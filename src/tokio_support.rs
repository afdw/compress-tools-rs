use crate::{Ownership, Result};
use futures::{
    channel::mpsc::{Receiver, Sender},
    join,
    stream::FusedStream,
    SinkExt, StreamExt,
};
use std::{
    future::Future,
    io::{Read, Write},
    path::Path,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    task::JoinError,
};

struct AsyncReadWrapper {
    rx: Receiver<u8>,
}

impl Read for AsyncReadWrapper {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.rx.is_terminated() {
            return Ok(0);
        }
        for (i, u) in buf.iter_mut().enumerate() {
            match futures::executor::block_on(self.rx.next()) {
                Some(v) => *u = v,
                None => return Ok(i),
            }
        }
        Ok(buf.len())
    }
}

fn make_async_read_wrapper_and_worker<R>(
    mut read: R,
) -> (AsyncReadWrapper, impl Future<Output = ()>)
where
    R: AsyncRead + Unpin,
{
    let (mut tx, rx) = futures::channel::mpsc::channel(0);
    (AsyncReadWrapper { rx }, async move {
        let mut v = [0];
        while !tx.is_closed() && read.read(&mut v).await.unwrap() == 1 {
            if tx.send(v[0]).await.is_err() {
                break;
            }
        }
    })
}

struct AsyncWriteWrapper {
    tx: Sender<u8>,
}

impl Write for AsyncWriteWrapper {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        for (i, u) in buf.iter().enumerate() {
            if futures::executor::block_on(self.tx.send(*u)).is_err() {
                return Ok(i);
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn make_async_write_wrapper_and_worker<W>(
    mut write: W,
) -> (AsyncWriteWrapper, impl Future<Output = ()>)
where
    W: AsyncWrite + Unpin,
{
    let (tx, mut rx) = futures::channel::mpsc::channel(0);
    (AsyncWriteWrapper { tx }, async move {
        while let Some(v) = rx.next().await {
            write.write_all(&[v]).await.unwrap();
        }
    })
}

async fn wrap_async_read<R, F, T>(read: R, f: F) -> std::result::Result<T, JoinError>
where
    R: AsyncRead + Unpin,
    F: FnOnce(AsyncReadWrapper) -> T + Send + 'static,
    T: Send + 'static,
{
    let (async_read_wrapper, async_read_wrapper_worker) = make_async_read_wrapper_and_worker(read);
    let h = tokio::task::spawn_blocking(move || f(async_read_wrapper));
    Ok(join!(h, async_read_wrapper_worker).0?)
}

async fn wrap_async_read_and_write<R, W, F, T>(
    read: R,
    write: W,
    f: F,
) -> std::result::Result<T, JoinError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    F: FnOnce(AsyncReadWrapper, AsyncWriteWrapper) -> T + Send + 'static,
    T: Send + 'static,
{
    let (async_read_wrapper, async_read_wrapper_worker) = make_async_read_wrapper_and_worker(read);
    let (async_write_wrapper, async_write_wrapper_worker) =
        make_async_write_wrapper_and_worker(write);
    let h = tokio::task::spawn_blocking(move || f(async_read_wrapper, async_write_wrapper));
    Ok(join!(h, async_read_wrapper_worker, async_write_wrapper_worker).0?)
}

pub async fn list_archive_files<R>(source: R) -> Result<Vec<String>>
where
    R: AsyncRead + Unpin,
{
    wrap_async_read(source, crate::list_archive_files).await?
}

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
