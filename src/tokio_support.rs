use crate::{Ownership, Result};
use futures::{
    channel::mpsc::{Receiver, Sender},
    SinkExt, StreamExt,
};
use std::{
    io::{Read, Write},
    path::Path,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

struct Reader {
    rx: Receiver<u8>,
}

impl Reader {
    fn new<R>(mut source: R) -> Self
    where
        R: AsyncRead + Send + Unpin + 'static,
    {
        let (mut tx, rx) = futures::channel::mpsc::channel(0);
        tokio::spawn(async move {
            let mut v = [0];
            while source.read(&mut v).await.unwrap() == 1 {
                if tx.send(v[0]).await.is_err() {
                    break;
                }
            }
        });
        Self { rx }
    }
}

impl Read for Reader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        for (i, u) in buf.iter_mut().enumerate() {
            match futures::executor::block_on(self.rx.next()) {
                Some(v) => *u = v,
                None => return Ok(i),
            }
        }
        Ok(buf.len())
    }
}

struct Writer {
    tx: Sender<u8>,
}

impl Writer {
    fn new<R>(mut target: R) -> Self
    where
        R: AsyncWrite + Send + Unpin + 'static,
    {
        let (tx, mut rx) = futures::channel::mpsc::channel(0);
        tokio::spawn(async move {
            while let Some(v) = rx.next().await {
                target.write_all(&[v]).await.unwrap();
            }
        });
        Self { tx }
    }
}

impl Write for Writer {
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

pub async fn list_archive_files<R>(source: R) -> Result<Vec<String>>
where
    R: AsyncRead + Send + Unpin + 'static,
{
    tokio::task::spawn_blocking(move || crate::list_archive_files(Reader::new(source))).await?
}

pub async fn uncompress_data<R, W>(source: R, target: W) -> Result<usize>
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    tokio::task::spawn_blocking(move || {
        crate::uncompress_data(Reader::new(source), Writer::new(target))
    })
    .await?
}

pub async fn uncompress_archive<R>(
    source: R,
    dest: &'static Path,
    ownership: Ownership,
) -> Result<()>
where
    R: AsyncRead + Send + Unpin + 'static,
{
    tokio::task::spawn_blocking(move || {
        crate::uncompress_archive(Reader::new(source), dest, ownership)
    })
    .await?
}

pub async fn uncompress_archive_file<R, W>(
    source: R,
    target: W,
    path: &'static str,
) -> Result<usize>
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    tokio::task::spawn_blocking(move || {
        crate::uncompress_archive_file(Reader::new(source), Writer::new(target), path)
    })
    .await?
}
