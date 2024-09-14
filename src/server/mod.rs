//! Server utilities.

use self::{
    conn::auto::{Builder, HttpServerConnExec},
    listener::Listener,
    modify::{Modify, NoModify, ServeModify},
};
use crate::service::{TowerToHyperService, TowerToHyperServiceFuture};
use http::Request;
use hyper::{body::Incoming, rt::Executor};
use std::{error::Error as StdError, net::SocketAddr};
use tower::ServiceExt;

#[cfg(feature = "tokio")]
use {crate::rt::TokioExecutor, futures_util::future::BoxFuture, tokio::net::TcpListener};

pub mod conn;
mod executor;
mod listener;
pub mod modify;

/// Http server.
pub struct Server<L, E, M> {
    listener: L,
    executor: E,
    modifier: M,
    auto_conn: Builder<E>,
}

impl<L, E: Clone> Server<L, E, NoModify> {
    /// Create a new http server.
    pub fn new(listener: L, executor: E) -> Self {
        Self {
            listener,
            auto_conn: Builder::new(executor.clone()),
            executor,
            modifier: NoModify::new(),
        }
    }
}

impl<L, E, M> Server<L, E, M> {
    /// Get a mutable reference to http configuration.
    pub fn http_config(&mut self) -> &mut Builder<E> {
        &mut self.auto_conn
    }

    /// Serve the `MakeService`.
    pub async fn serve<MS, S>(&self, mut make_service: MS) -> std::io::Result<()>
    where
        L: Listener,
        L::Conn: Send + 'static,
        L::Error: Into<Box<dyn StdError + Send + Sync>>,
        MS: tower_service::Service<L::Data, Response = S>,
        MS::Error: Into<Box<dyn StdError + Send + Sync>>,
        S: Send + 'static,
        M: ServeModify<L::Conn, S>,
        E: Executor<BoxFuture<'static, ()>>
            + HttpServerConnExec<
                TowerToHyperServiceFuture<M::ServeService, Request<Incoming>>,
                M::ServiceBody,
            > + Send
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
            let auto_conn = self.auto_conn.clone();
            let modifier = self.modifier.clone().into_modify();

            self.executor.execute(Box::pin(async move {
                let (conn, service) = match modifier.modify(conn, service).await {
                    Ok(v) => v,
                    Err(_) => return,
                };
                let service = TowerToHyperService::new(service);

                let _ = auto_conn
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

#[cfg(test)]
mod tests {
    use super::Server;
    use crate::rt::{TokioExecutor, TokioIo};
    use bytes::Bytes;
    use http::{Request, Response, StatusCode};
    use http_body::Body;
    use http_body_util::Empty;
    use hyper::{body::Incoming, client::conn::http1 as client_http1};
    use std::{convert::Infallible, error::Error as StdError, net::SocketAddr};
    use tokio::net::{TcpListener, TcpStream};
    use tower::{make::Shared, service_fn};

    #[cfg(not(miri))]
    #[tokio::test]
    #[allow(unused_must_use)]
    async fn request() {
        let addr = start_server().await;
        let mut client = connect::<Empty<Bytes>>(addr).await;

        client.ready().await.unwrap();
        let response = client
            .send_request(Request::new(Empty::new()))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK)
    }

    async fn connect<B>(addr: SocketAddr) -> client_http1::SendRequest<B>
    where
        B: Body + Send + 'static,
        B::Data: Send,
        B::Error: Into<Box<dyn StdError + Send + Sync>>,
    {
        let io = TokioIo::new(TcpStream::connect(addr).await.unwrap());
        let (send_request, connection) = client_http1::handshake(io).await.unwrap();

        tokio::spawn(connection);

        send_request
    }

    async fn start_server() -> SocketAddr {
        let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            Server::new(listener, TokioExecutor::new())
                .serve(Shared::new(service_fn(
                    |_request: Request<Incoming>| async {
                        Ok::<_, Infallible>(Response::new(Empty::<Bytes>::new()))
                    },
                )))
                .await
                .unwrap();
        });

        addr
    }
}
