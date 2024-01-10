// use base64::Engine;
// use base64::alphabet::URL_SAFE;
// use base64::engine::general_purpose::PAD;
// use base64::engine::GeneralPurpose;
// use sha2::{Digest, Sha256};
// use std::pin::Pin;
// use std::task::{Context, Poll};
// use tokio::io::{self, AsyncRead, ReadBuf, AsyncReadExt};
//
// pub const URL_SAFE_ENGINE: GeneralPurpose = GeneralPurpose::new(&URL_SAFE, PAD);
// pub struct BlobStreamReader<R> {
//   reader: R,
//   hasher: Sha256,
// }
//
// impl<R> AsyncRead for BlobStreamReader<R>
// where
//   R: AsyncRead + Unpin,
// {
//   fn poll_read(
//     mut self: Pin<&mut Self>,
//     cx: &mut Context<'_>,
//     buf: &mut ReadBuf<'_>,
//   ) -> Poll<io::Result<()>> {
//     let before = buf.filled().len();
//     let poll = Pin::new(&mut self.reader).poll_read(cx, buf);
//     let after = buf.filled().len();
//     if after > before {
//       self.hasher.update(&buf.filled()[before..after]);
//     }
//     poll
//   }
// }
//
// impl<R> BlobStreamReader<R>
// where
//   R: AsyncRead + Unpin,
// {
//   pub fn new(reader: R) -> Self {
//     Self {
//       reader,
//       hasher: Sha256::new(),
//     }
//   }
//
//   pub async fn finish(mut self) -> io::Result<(Vec<u8>, String)> {
//     let mut buffer = Vec::new();
//     let _ = self.read_to_end(&mut buffer).await?;
//     let hash = URL_SAFE_ENGINE.encode(self.hasher.finalize());
//     Ok((buffer, hash))
//   }
// }
//
// impl<R> AsRef<R> for BlobStreamReader<R>
// where
//   R: AsyncRead + Unpin,
// {
//   fn as_ref(&self) -> &R {
//     &self.reader
//   }
// }
