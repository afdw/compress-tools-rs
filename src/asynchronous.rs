use crate::{
    error::Result,
    base::{READER_BUFFER_SIZE, ArchiveReader},
    Mode,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};

struct AsyncPipe<'a> {
    reader: &'a mut (dyn AsyncRead + Unpin),
    buffer: [u8; READER_BUFFER_SIZE],
}

impl AsyncPipe<'_> {
    fn from<R>(reader: &mut R) -> AsyncPipe where R: AsyncRead + Unpin {
        AsyncPipe {
            reader,
            buffer: [0; READER_BUFFER_SIZE]
        }
    }
}

impl crate::base::Pipe for AsyncPipe<'_> {
    fn buffer(&mut self) -> *const std::os::raw::c_void {
        self.buffer.as_ptr() as *const std::os::raw::c_void
    }

    fn read(&mut self) -> std::result::Result<usize, Box<dyn std::error::Error>> {
        Ok(futures::executor::block_on(self.reader.read(&mut self.buffer))?)
    }
}

/// Get all files in a archive using `source` as a reader.
/// # Example
///
/// ```no_run
/// # async fn test() -> Result<(), Box<dyn std::error::Error>> {
/// use compress_tools::asynchronous::*;
/// use tokio::fs::File;
///
/// let mut source = File::open("tree.tar").await?;
///
/// let file_list = list_archive_files(&mut source).await?;
/// # Ok(())
/// # }
/// ```
pub async fn list_archive_files<R>(mut source: R) -> Result<Vec<String>>
where
    R: AsyncRead + Unpin,
{
    let pipe = AsyncPipe::from(&mut source);
    return ArchiveReader::read_all_entries(pipe);
}

/// Uncompress a file using the `source` need as reader and the `target` as a
/// writer.
///
/// # Example
///
/// ```no_run
/// # async fn test() -> Result<(), Box<dyn std::error::Error>> {
/// use compress_tools::asynchronous::*;
/// use tokio::fs::File;
///
/// let mut source = File::open("file.txt.gz").await?;
/// let mut target = Vec::default();
///
/// uncompress_data(&mut source, &mut target).await?;
/// # Ok(())
/// # }
/// ```
pub async fn uncompress_data<R, W>(mut source: R, target: W) -> Result<usize>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let pipe = AsyncPipe::from(&mut source);
    ArchiveReader::open_with_async(Mode::RawFormat, pipe, |archive| async {
        if let Err(err) = archive.next_entry() {
            return (Err(err), archive);
        }
        (archive.read_into_async(target).await, archive)
    }).await
}

/// Uncompress a specific file from an archive. The `source` is used as a
/// reader, the `target` as a writer and the `path` is the relative path for
/// the file to be extracted from the archive.
///
/// # Example
///
/// ```no_run
/// # async fn test() -> Result<(), Box<dyn std::error::Error>> {
/// use compress_tools::asynchronous::*;
/// use tokio::fs::File;
///
/// let mut source = File::open("tree.tar.gz").await?;
/// let mut target = Vec::default();
///
/// uncompress_archive_file(&mut source, &mut target, "file/path").await?;
/// # Ok(())
/// # }
/// ```
pub async fn uncompress_archive_file<R, W>(mut source: R, target: W, path: &str) -> Result<usize>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let pipe = AsyncPipe::from(&mut source);
    ArchiveReader::open_with_async(Mode::RawFormat, pipe, |archive| async {
        if let Err(err) = archive.find_entry(path) {
            return (Err(err), archive);
        }
        (archive.read_into_async(target).await, archive)
    }).await
}