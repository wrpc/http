use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context as _;
use clap::Parser;
use futures::stream::StreamExt as _;
use http::uri::Uri;
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::sync::mpsc;
use tracing::error;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use wrpc_interface_http::bindings::wrpc::http::types::ErrorCode;
use wrpc_interface_http::InvokeIncomingHandler;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// NATS.io URL to connect to
    #[arg(short, long, default_value = "nats://127.0.0.1:4222")]
    nats: Uri,

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

    let nats = connect(nats)
        .await
        .context("failed to connect to NATS.io")?;
    let wrpc = wrpc_transport_nats::Client::new(nats, prefix, None)
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
                .invoke_handle_http(None, req)
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

/// Connect to NATS.io server and ensure that the connection is fully established before
/// returning the resulting [`async_nats::Client`]
async fn connect(url: Uri) -> anyhow::Result<async_nats::Client> {
    let (conn_tx, mut conn_rx) = mpsc::channel(1);
    let client = async_nats::connect_with_options(
        url.to_string(),
        async_nats::ConnectOptions::new()
            .retry_on_initial_connect()
            .event_callback(move |event| {
                let conn_tx = conn_tx.clone();
                async move {
                    if let async_nats::Event::Connected = event {
                        conn_tx
                            .send(())
                            .await
                            .expect("failed to send NATS.io server connection notification");
                    }
                }
            }),
    )
    .await
    .context("failed to connect to NATS.io server")?;
    conn_rx
        .recv()
        .await
        .context("failed to await NATS.io server connection to be established")?;
    Ok(client)
}
