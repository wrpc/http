use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context as _;
use bytes::Bytes;
use clap::Parser;
use futures::stream::StreamExt as _;
use hyper_util::rt::{TokioExecutor, TokioIo};
use tracing::error;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use wrpc_interface_http::bindings::wrpc::http::types::ErrorCode;
use wrpc_interface_http::InvokeIncomingHandler;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// NATS.io URL to connect to
    #[arg(short, long, default_value = "127.0.0.1:4222")]
    nats: SocketAddr,

    /// Prefix to send `wrpc:http/incoming-handler` invocations on
    #[arg(short, long, default_value = "rust")]
    prefix: String,

    /// Address to listen on
    addr: SocketAddr,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().compact().without_time())
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let Args { nats, prefix, addr } = Args::parse();

    let (nats, _) = ants::Conn::connect_tcp(nats)
        .await
        .context("failed to connect to NATS.io server")?;
    let (nats, io) = ants::Client::new(nats, ants::protocol::ConnectOptions::default())
        .await
        .context("failed to establish NATS.io protocol connection")?;
    let conn = tokio::spawn(async move {
        if let Err(err) = io.await {
            error!(?err, "connection failed");
        }
    });
    let wrpc = wrpc_transport_ants::Client::new(nats)
        .await
        .context("failed to construct transport client")?;
    let wrpc = Arc::new(wrpc);

    let socket = match &addr {
        SocketAddr::V4(_) => tokio::net::TcpSocket::new_v4()?,
        SocketAddr::V6(_) => tokio::net::TcpSocket::new_v6()?,
    };
    socket.set_reuseaddr(!cfg!(windows))?;
    socket.bind(addr)?;
    let listener = socket.listen(1024)?;

    let srv = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new());
    let srv = Arc::new(srv);

    let svc = hyper::service::service_fn(move |req| {
        let wrpc = Arc::clone(&wrpc);
        async move {
            let (res, errs, io) = wrpc
                .invoke_handle_http(Bytes::default(), req)
                .await
                .map_err(|err| ErrorCode::InternalError(Some(format!("{err:#}"))))?;
            if let Some(io) = io {
                tokio::spawn(async move {
                    if let Err(err) = io.await {
                        error!(?err, "failed to handle async I/O")
                    }
                });
            }
            tokio::spawn(
                errs.for_each(|err| async move { error!(?err, "body error encountered") }),
            );
            res
        }
    });

    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn({
            let srv = Arc::clone(&srv);
            let svc = svc.clone();
            async move {
                if let Err(err) = srv.serve_connection(TokioIo::new(stream), svc).await {
                    error!(?err, "failed to serve connection");
                }
            }
        });
    }
}
