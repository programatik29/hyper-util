//! Modify trait

use crate::sealed::Sealed;
use http::{Request, Response};
use http_body::Body;
use hyper::{
    body::Incoming,
    rt::{Read, Write},
};
use std::{convert::Infallible, error::Error as StdError, future::Future};

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
#[derive(Debug, Default, Clone)]
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

/// [`Modify`] trait bounds for [`serve`](super::Server::serve) function.
///
/// This trait is sealed and cannot be implemented for types outside this crate.
#[allow(missing_docs)]
pub trait ServeModify<I, S>: Sealed<(I, S)> + Clone {
    #[doc(hidden)]
    type Modify: for<'a> Modify<
            I,
            S,
            Conn = Self::ServeConn,
            Service = Self::ServeService,
            Error = Self::ServeError,
            Future<'a> = Self::ServeFuture<'a>,
        > + Send
        + 'static;

    type ServeConn: Read + Write + Unpin + Send + 'static;
    type ServeService: tower_service::Service<
            Request<Incoming>,
            Response = Response<Self::ServiceBody>,
            Future = Self::ServiceFuture,
            Error = Self::ServiceError,
        > + Clone
        + Send
        + 'static;
    type ServeError: Into<Box<dyn StdError + Send + Sync>>;
    type ServeFuture<'a>: Future<Output = Result<(Self::ServeConn, Self::ServeService), Self::ServeError>>
        + Send
        + 'a;

    type ServiceFuture: Send + 'static;
    type ServiceError: Into<Box<dyn StdError + Send + Sync>>;
    type ServiceBody: Body<Data = Self::ServiceBodyData, Error = Self::ServiceBodyError>
        + Send
        + 'static;

    type ServiceBodyData: Send;
    type ServiceBodyError: Into<Box<dyn StdError + Send + Sync>>;

    #[doc(hidden)]
    fn into_modify(self) -> Self::Modify;
}

impl<T, Svc, B, I, S> Sealed<(I, S)> for T
where
    T: Modify<I, S, Service = Svc> + Clone + Send + 'static,
    T::Conn: Read + Write + Unpin + Send + 'static,
    Svc: tower_service::Service<Request<Incoming>, Response = Response<B>> + Clone + Send + 'static,
    Svc::Future: Send + 'static,
    Svc::Error: Into<Box<dyn StdError + Send + Sync>>,
    T::Error: Into<Box<dyn StdError + Send + Sync>>,
    for<'a> T::Future<'a>: std::marker::Send,
    B: Body + Send + 'static,
    B::Data: Send,
    B::Error: Into<Box<dyn StdError + Send + Sync>>,
{
}

impl<T, Svc, B, I, S> ServeModify<I, S> for T
where
    T: Modify<I, S, Service = Svc> + Clone + Send + 'static,
    T::Conn: Read + Write + Unpin + Send + 'static,
    Svc: tower_service::Service<Request<Incoming>, Response = Response<B>> + Clone + Send + 'static,
    Svc::Future: Send + 'static,
    Svc::Error: Into<Box<dyn StdError + Send + Sync>>,
    T::Error: Into<Box<dyn StdError + Send + Sync>>,
    for<'a> T::Future<'a>: std::marker::Send,
    B: Body + Send + 'static,
    B::Data: Send,
    B::Error: Into<Box<dyn StdError + Send + Sync>>,
{
    type Modify = T;

    type ServeConn = T::Conn;
    type ServeService = Svc;
    type ServeError = T::Error;
    type ServeFuture<'a> = T::Future<'a>;

    type ServiceFuture = Svc::Future;
    type ServiceError = Svc::Error;
    type ServiceBody = B;

    type ServiceBodyData = B::Data;
    type ServiceBodyError = B::Error;

    fn into_modify(self) -> Self::Modify {
        self
    }
}
