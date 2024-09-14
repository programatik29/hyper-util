use crate::rt::TokioIo;
use futures_util::{future::BoxFuture, TryFutureExt};
use std::{future::Future, net::SocketAddr};
use tokio::net::{TcpListener, TcpStream};

/// Asynchronously accept incoming connections.
pub trait Listener {
    /// The connection type that can be accepted.
    type Conn;

    /// Data related to accepted connection. Usually the address.
    type Data;

    /// The error type that can occur when accepting a connection.
    type Error;

    /// Future type.
    type Future<'a>: Future<Output = Result<(Self::Conn, Self::Data), Self::Error>> + 'a
    where
        Self: 'a;

    /// Accept next connection.
    fn accept(&self) -> Self::Future<'_>;
}

#[cfg(feature = "tokio")]
impl Listener for TcpListener {
    type Conn = TokioIo<TcpStream>;
    type Data = SocketAddr;
    type Error = std::io::Error;
    type Future<'a> = BoxFuture<'a, std::io::Result<(Self::Conn, Self::Data)>>;

    fn accept(&self) -> Self::Future<'_> {
        Box::pin(self.accept().map_ok(|(c, d)| (TokioIo::new(c), d)))
    }
}
