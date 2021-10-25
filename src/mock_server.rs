use log::debug;
use prost::Message;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    sync::{Arc, RwLock},
    task::Poll,
    time::Duration,
};
use tonic::{
    codegen::{http, Body, Never, StdError},
    Code, Status,
};

#[derive(Clone)]
pub struct MockGrpcServer {
    address: SocketAddr,
    inner: Arc<Option<Inner>>,
    rules: Arc<RwLock<Vec<RequestBuilder>>>,
}

struct Inner {
    #[allow(dead_code)]
    join_handle: tokio::task::JoinHandle<Result<(), tonic::transport::Error>>,
}

impl MockGrpcServer {
    pub fn new(port: u16) -> Self {
        Self {
            address: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port),
            inner: Arc::default(),
            rules: Arc::default(),
        }
    }

    pub async fn start(mut self) -> Self {
        println!("Starting gRPC started in {}", self.address());

        let thread = tokio::spawn(
            tonic::transport::Server::builder()
                .add_service(self.clone())
                .serve(self.address),
        );

        for _ in 0..40 {
            if TcpStream::connect_timeout(&self.address, std::time::Duration::from_millis(25))
                .is_ok()
            {
                break;
            }
            debug!("WAITING...");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }


        self.inner = Arc::new(Some(Inner {
            join_handle: thread,
        }));

        println!("Server started in {}", self.address());
        self
    }

    pub fn setup(&mut self, r: RequestBuilder) -> MockGrpcServer {
        r.mount(self);

        self.to_owned()
    }

    pub fn address(&self) -> &SocketAddr {
        &self.address
    }
}

impl tonic::transport::NamedService for MockGrpcServer {
    const NAME: &'static str = "wiremock.server";
}

impl<B> tonic::codegen::Service<http::Request<B>> for MockGrpcServer
where
    B: Body + Send + 'static,
    B::Error: Into<StdError> + Send + 'static,
{
    type Response = http::Response<tonic::body::BoxBody>;
    type Error = Never;
    type Future = tonic::codegen::BoxFuture<Self::Response, Self::Error>;

    fn poll_ready(&mut self, _cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: http::Request<B>) -> Self::Future {
        println!("Request to {}", req.uri().path());

        let path = req.uri().path();
        let inner = self.rules.as_ref();
        if let Some(req_builder) = inner.read().unwrap().iter().find(|x| x.path == path) {
            let builder = http::Response::builder()
                .status(200)
                .header("content-type", "application/grpc")
                .header(
                    "grpc-status",
                    Status::new(req_builder.status_code.unwrap_or(Code::Ok), "").to_string(),
                );

            if let Some(body) = &req_builder.result {
                builder.body(body).unwrap();
            } else {
                builder.body(tonic::body::empty_body()).unwrap();
            }
        }

        Box::pin(async move {
            Ok(http::Response::builder()
                .status(200)
                .header("grpc-status", "12")
                .header("content-type", "application/grpc")
                .body(tonic::body::empty_body())
                .unwrap())
        })
    }
}

pub struct RequestBuilder {
    path: String,
    status_code: Option<tonic::Code>,
    result: Option<Vec<u8>>,
}

impl RequestBuilder {
    pub fn given(path: &str) -> Self {
        Self {
            path: path.into(),
            result: None,
            status_code: None,
        }
    }

    pub fn when(&self) -> Self {
        todo!()
    }

    pub fn return_status(self, status: tonic::Code) -> Self {
        Self {
            status_code: Some(status),
            ..self
        }
    }

    pub fn return_body<T, F>(self, f: F) -> Self
    where
        F: Fn() -> T,
        T: prost::Message,
    {
        let result = f();
        let result = result.encode_to_vec();

        Self {
            result: Some(result),
            ..self
        }
    }

    pub fn mount(self, s: &mut MockGrpcServer) {
        if self.status_code.is_none() && self.result.is_none() {
            panic!("Must set the status code or body before attempting to mount the rule.");
        }

        s.rules.write().unwrap().push(self);
    }
}