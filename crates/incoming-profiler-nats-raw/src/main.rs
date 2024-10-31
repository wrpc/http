use core::pin::pin;

use std::net::SocketAddr;

use anyhow::{ensure, Context as _};
use bytes::Bytes;
use clap::Parser;
use futures::StreamExt as _;
use http::uri::Uri;
use http_body_util::Full;
use hyper_util::rt::{TokioExecutor, TokioIo};
use pprof::protos::Message as _;
use tokio::sync::mpsc;
use tokio::{select, signal};
use tracing::{debug, error};
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

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

    #[arg(short, long, default_value_t = false)]
    fast: bool,

    body: String,
}

async fn handle_request_fast(
    ac: ants::Client,
    subject: Bytes,
    inbox: Bytes,
    _request: http::Request<hyper::body::Incoming>,
) -> anyhow::Result<http::Response<Full<Bytes>>> {
    let (tx, mut rx) = mpsc::channel(1);
    ac.subscribe(inbox.clone(), Bytes::default(), inbox.clone(), tx)
        .await
        .context("failed to subscribe")?;
    ac.publish(subject, inbox, Bytes::default(), Bytes::default())
        .await
        .context("failed to publish")?;
    let ants::Message { payload, .. } = rx.recv().await.context("failed to recv")?;
    Ok(http::Response::new(Full::new(payload)))
}

async fn handle_request_slow(
    nats: async_nats::Client,
    subject: async_nats::Subject,
    _request: http::Request<hyper::body::Incoming>,
) -> anyhow::Result<http::Response<Full<Bytes>>> {
    let async_nats::Message {
        payload, status, ..
    } = nats
        .send_request(subject, async_nats::Request::new())
        .await
        .context("failed to send request")?;
    ensure!(status.is_none());
    Ok(http::Response::new(Full::new(payload)))
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
        fast,
        body,
    } = Args::parse();
    let handle_request = move |nats, ac, subject, inbox, req| async move {
        if fast {
            handle_request_fast(ac, Bytes::from(subject), inbox, req).await
        } else {
            handle_request_slow(nats, async_nats::Subject::from(subject), req).await
        }
    };

    let body = Bytes::from(body);

    let profile = profile
        .map(|profile| pprof::ProfilerGuard::new(100).map(|p| (p, profile)))
        .transpose()
        .context("failed to build profiler")?;

    let (ac, io) = ants::Client::new("127.0.0.1:4222", ants::DefaultConnector)
        .await
        .context("failed to connect to NATS.io server")?;
    tokio::spawn(async move {
        if let Err(err) = io.await {
            error!(?err, "connection failed")
        }
    });

    let nats = async_nats::connect_with_options(
        nats.to_string(),
        async_nats::ConnectOptions::new().retry_on_initial_connect(),
    )
    .await
    .context("failed to connect to NATS.io server")?;

    let subject = format!("{prefix}.wrpc.0.0.1.wrpc:http/incoming-handler@0.1.0.handle");

    let mut sub = nats
        .subscribe(subject.clone())
        .await
        .context("failed to subscribe")?;

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
            Some(async_nats::Message { reply, status, .. }) = sub.next() => {
                debug!("invocation accepted");
                let body = body.clone();
                let nats = nats.clone();
                tokio::spawn(async move {
                    let reply = reply.context("reply missing")?;
                    ensure!(status.is_none());
                    nats.publish(reply, body).await.context("failed to publish")
                });
            }

            res = listener.accept() => {
                let (stream, _) = res?;
                tokio::spawn({
                    let nats = nats.clone();
                    let ac = ac.clone();
                    let srv = srv.clone();
                    let subject = subject.clone();
                    let inbox = Bytes::from(nats.new_inbox());
                    async move {
                        if let Err(err) = srv.serve_connection(TokioIo::new(stream), hyper::service::service_fn(move |req| handle_request(nats.clone(), ac.clone(), subject.clone(), inbox.clone(), req))).await {
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
