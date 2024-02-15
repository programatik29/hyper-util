//! Server utilities.

use self::conn::auto::{Builder, HttpServerConnExec};
use crate::service::{TowerToHyperService, TowerToHyperServiceFuture};
use http::{Request, Response};
use http_body::Body;
use hyper::{
    body::Incoming,
    rt::{Executor, Read, Write},
};
use std::{convert::Infallible, error::Error as StdError, future::Future, net::SocketAddr};
use tower::ServiceExt;

#[cfg(feature = "tokio")]
use {
    crate::rt::TokioExecutor,
    futures_util::future::BoxFuture,
    tokio::net::{TcpListener, TcpStream},
};

pub mod conn;

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
    type Conn = TcpStream;
    type Data = SocketAddr;
    type Error = std::io::Error;
    type Future<'a> = BoxFuture<'a, std::io::Result<(Self::Conn, Self::Data)>>;

    fn accept(&self) -> Self::Future<'_> {
        Box::pin(self.accept())
    }
}

/// Asynchronously modify connection and service.
pub trait Modify<I, S> {
    /// New connection type.
    type Conn;

    /// New service type.
    type Service;

    /// The error type that can occur during modification.
    type Error;

    /// Future type.
    type Future<'a>: Future<Output = Result<(Self::Conn, Self::Service), Self::Error>> + 'a
    where
        Self: 'a;

    /// Modify connection and service.
    fn modify(&self, conn: I, service: S) -> Self::Future<'_>;
}

/// Default modifier for server.
#[derive(Debug)]
pub struct NoModify {}

impl NoModify {
    /// Create a new `NoModify`.
    pub fn new() -> Self {
        Self {}
    }
}

impl<I: 'static, S: 'static> Modify<I, S> for NoModify {
    type Conn = I;
    type Service = S;
    type Error = Infallible;
    type Future<'a> = std::future::Ready<Result<(Self::Conn, Self::Service), Infallible>>;

    fn modify(&self, conn: I, service: S) -> Self::Future<'_> {
        std::future::ready(Ok((conn, service)))
    }
}

/// Http server.
pub struct Server<L, E, M> {
    listener: L,
    executor: E,
    modifier: M,
    conn_builder: Builder<E>,
}

impl<L, E: Clone> Server<L, E, NoModify> {
    /// Create a new http server.
    pub fn new(listener: L, executor: E) -> Self {
        Self {
            listener,
            conn_builder: Builder::new(executor.clone()),
            executor,
            modifier: NoModify::new(),
        }
    }
}

impl<L, E, M> Server<L, E, M> {
    /// Get a mutable reference to http configuration.
    pub fn http_config(&mut self) -> &mut Builder<E> {
        &mut self.conn_builder
    }

    /// Serve the `MakeService`.
    pub async fn serve<MS, S, B>(&self, mut make_service: MS) -> std::io::Result<()>
    where
        L: Listener,
        L::Conn: Read + Write + Unpin + Send + 'static,
        L::Error: Into<Box<dyn StdError + Send + Sync>>,
        MS: tower_service::Service<L::Data, Response = S>,
        MS::Error: Into<Box<dyn StdError + Send + Sync>> + Clone,
        S: Send + 'static,
        M: Modify<L::Conn, S> + Clone + Send + 'static,
        M::Conn: Read + Write + Unpin + Send + 'static,
        M::Service: tower_service::Service<Request<Incoming>, Response = Response<B>>
            + Clone
            + Send
            + 'static,
        <M::Service as tower_service::Service<Request<Incoming>>>::Future: Send + 'static,
        <M::Service as tower_service::Service<Request<Incoming>>>::Error:
            Into<Box<dyn StdError + Send + Sync>>,
        M::Error: Into<Box<dyn StdError + Send + Sync>>,
        for<'a> M::Future<'a>: std::marker::Send,
        B: Body + Send + 'static,
        B::Data: Send,
        B::Error: Into<Box<dyn StdError + Send + Sync>>,
        E: Executor<BoxFuture<'static, ()>>
            + HttpServerConnExec<TowerToHyperServiceFuture<M::Service, Request<Incoming>>, B>
            + Send
            + Sync
            + 'static,
    {
        loop {
            let (conn, addr) = self.listener.accept().await.map_err(io_other)?;
            let service = make_service
                .ready()
                .await
                .map_err(io_other)?
                .call(addr)
                .await
                .map_err(io_other)?;
            let conn_builder = self.conn_builder.clone();
            let modifier = self.modifier.clone();

            self.executor.execute(Box::pin(async move {
                let (conn, service) = match modifier.modify(conn, service).await {
                    Ok(v) => v,
                    Err(_) => return,
                };
                let service = TowerToHyperService::new(service);

                let _ = conn_builder
                    .serve_connection_with_upgrades(conn, service)
                    .await;
            }));
        }
    }
}

#[cfg(feature = "tokio")]
impl Server<TcpListener, TokioExecutor, NoModify> {
    /// Bind to address and create http server with default configuration.
    pub async fn bind(addr: SocketAddr) -> std::io::Result<Self> {
        let listener = TcpListener::bind(addr).await?;

        Ok(Self::new(listener, TokioExecutor::new()))
    }
}

fn io_other(e: impl Into<Box<dyn StdError + Send + Sync>>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, e)
}
