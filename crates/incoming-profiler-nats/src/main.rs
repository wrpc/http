use core::pin::pin;

use std::net::SocketAddr;

use anyhow::Context as _;
use bytes::Bytes;
use clap::Parser;
use futures::StreamExt as _;
use http::uri::Uri;
use http_body_util::Full;
use hyper_util::rt::{TokioExecutor, TokioIo};
use pprof::protos::Message as _;
use tokio::{select, signal};
use tracing::{debug, error, instrument, warn};
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use wrpc_interface_http::bindings::exports::wrpc::http::incoming_handler;
use wrpc_interface_http::bindings::wrpc::http::types::ErrorCode;
use wrpc_interface_http::{HttpBody, InvokeIncomingHandler as _, ServeHttp};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// NATS.io URL to connect to
    #[arg(short, long, default_value = "nats://127.0.0.1:4222")]
    nats: Uri,

    /// Prefix to serve `wrpc:http/incoming-handler` invocations on
    #[arg(short, long, default_value = "rust")]
    prefix: String,

    /// Address to listen on
    #[arg(short, long, default_value = "127.0.0.1:3004")]
    addr: SocketAddr,

    /// If specified, `pprof` profile will be output at the path
    #[arg(long)]
    profile: Option<String>,

    body: String,
}

#[derive(Clone)]
pub struct Handler(Bytes);

impl<T: Send> wrpc_interface_http::ServeIncomingHandlerHttp<T> for Handler {
    #[instrument(level = "trace", skip_all)]
    async fn handle(
        &self,
        _cx: T,
        _req: hyper::Request<HttpBody>,
    ) -> anyhow::Result<
        Result<
            hyper::Response<
                impl http_body::Body<Data = Bytes, Error = core::convert::Infallible> + Send + 'static,
            >,
            ErrorCode,
        >,
    > {
        Ok(Ok(hyper::Response::new(Full::new(self.0.clone()))))
    }
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

    let Args {
        nats,
        prefix,
        addr,
        profile,
        body,
    } = Args::parse();

    let profile = profile
        .map(|profile| pprof::ProfilerGuard::new(100).map(|p| (p, profile)))
        .transpose()
        .context("failed to build profiler")?;

    let nats = async_nats::connect_with_options(
        nats.to_string(),
        async_nats::ConnectOptions::new().retry_on_initial_connect(),
    )
    .await
    .context("failed to connect to NATS.io server")?;
    let wrpc = wrpc_transport_nats::Client::new(nats, prefix, None)
        .await
        .context("failed to construct NATS.io transport client")?;

    let [(_, _, mut invocations)] =
        incoming_handler::serve_interface(&wrpc, ServeHttp(Handler(Bytes::from(body))))
            .await
            .context("failed to serve `wrpc:http/incoming-handler` interface")?;

    let socket = match &addr {
        SocketAddr::V4(_) => tokio::net::TcpSocket::new_v4()?,
        SocketAddr::V6(_) => tokio::net::TcpSocket::new_v6()?,
    };
    socket.set_reuseaddr(!cfg!(windows))?;
    socket.bind(addr)?;
    let listener = socket.listen(1024)?;

    let srv = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new());

    let shutdown = signal::ctrl_c();
    let mut shutdown = pin!(shutdown);
    loop {
        select! {
            Some(res) = invocations.next() => {
                match res {
                    Ok(inv) => {
                        debug!("invocation accepted");
                        tokio::spawn(async move {
                            match inv.await {
                                Ok(()) => debug!("invocation successfully handled"),
                                Err(err) => warn!(?err, "failed to handle invocation"),
                            }
                        });
                    }
                    Err(err) => {
                        warn!(?err, "failed to accept invocation");
                    }
                }
            }

            res = listener.accept() => {
                let (stream, _) = res?;
                tokio::spawn({
                    let wrpc = wrpc.clone();
                    let srv = srv.clone();
                    async move {
                        if let Err(err) = srv.serve_connection(TokioIo::new(stream), hyper::service::service_fn(move |req| {
                            let wrpc = wrpc.clone();
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
                        })).await {
                            error!(?err, "failed to serve connection");
                        }
                    }
                });
            }

            res = &mut shutdown => {
                if let Some((profile, path)) = profile {
                    let profile = profile.report().build().context("failed to build performance profile")?;
                    let profile = profile.pprof().context("failed to get `pprof`")?;
                    let mut file = std::fs::File::create(path).context("failed to create profile")?;
                    profile.write_to_writer(&mut file).context("failed to write profile")?;
                }
                return res.context("failed to listen for ^C")
            }
        }
    }
}
