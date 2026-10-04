use tokio::io::{AsyncRead, AsyncWrite};

/// A live, non-replayable byte stream. The service owns routing and resource lifetime.
pub trait ByteStream: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> ByteStream for T {}
pub type Stream = Box<dyn ByteStream>;
