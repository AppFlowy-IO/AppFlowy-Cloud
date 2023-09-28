use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{self, AsyncRead, ReadBuf};

pub struct CountingReader<R> {
  reader: R,
  count: usize,
}

impl<R> AsyncRead for CountingReader<R>
where
  R: AsyncRead + Unpin,
{
  fn poll_read(
    mut self: Pin<&mut Self>,
    cx: &mut Context<'_>,
    buf: &mut ReadBuf<'_>,
  ) -> Poll<io::Result<()>> {
    let before = buf.filled().len();
    let poll = Pin::new(&mut self.reader).poll_read(cx, buf);
    let after = buf.filled().len();
    self.count += after - before;
    poll
  }
}

impl<R> CountingReader<R> {
  pub fn new(reader: R) -> Self {
    Self { reader, count: 0 }
  }

  pub fn count(&self) -> usize {
    self.count
  }
}

impl<R> AsRef<R> for CountingReader<R>
where
  R: AsyncRead + Unpin,
{
  fn as_ref(&self) -> &R {
    &self.reader
  }
}
