use core::convert::Infallible;

use anyhow::Context as _;
use bytes::Bytes;
use clap::Parser;
use http::uri::Uri;
use http_body_util::{BodyExt as _, StreamBody};
use hyper_util::rt::TokioExecutor;
use tokio::signal;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, error, instrument, warn};
use wrpc_interface_http::bindings::exports::wrpc::http::outgoing_handler;
use wrpc_interface_http::bindings::wrpc::http::types::{ErrorCode, RequestOptions};
use wrpc_interface_http::{HttpBody, ServeHttp, ServeOutgoingHandlerHttp};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// NATS.io URL to connect to
    #[arg(short, long, default_value = "nats://127.0.0.1:4222")]
    nats: Uri,

    /// Prefix to serve `wrpc:http/ougoing-handler.handle` on
    #[arg(default_value = "rust")]
    prefix: String,
}

#[derive(Clone)]
pub struct Handler(
    hyper_util::client::legacy::Client<
        hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>,
        HttpBody,
    >,
);

impl ServeOutgoingHandlerHttp<Option<async_nats::HeaderMap>> for Handler {
    #[instrument(level = "trace", skip_all)]
    async fn handle(
        &self,
        _cx: Option<async_nats::HeaderMap>,
        request: http::Request<HttpBody>,
        options: Option<RequestOptions>,
    ) -> anyhow::Result<
        Result<
            http::Response<
                impl http_body::Body<Data = Bytes, Error = Infallible> + Send + Sync + 'static,
            >,
            ErrorCode,
        >,
    > {
        // TODO: Use options
        let _ = options;
        debug!(uri = ?request.uri(), "sending HTTP request");
        let res = self
            .0
            .request(request)
            .await
            .map_err(|err| ErrorCode::InternalError(Some(err.to_string())))?;

        Ok(Ok(res.map(|mut body| {
            let (tx, rx) = mpsc::channel(1);
            // Handle errors encountered while reading the body
            tokio::spawn(async move {
                while let Some(frame) = body.frame().await {
                    match frame {
                        Ok(frame) => {
                            if let Err(err) = tx.send(Ok(frame)).await {
                                error!(?err, "failed to send frame");
                                return;
                            }
                        }
                        Err(err) => {
                            error!(?err, "body error encountered");
                            return;
                        }
                    }
                }
            });
            StreamBody::new(ReceiverStream::new(rx))
        })))
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();

    let Args { nats, prefix } = Args::parse();

    let native_roots = match rustls_native_certs::load_native_certs() {
        Ok(certs) => certs,
        Err(err) => {
            warn!(?err, "failed to load native root certificate store");
            vec![]
        }
    };
    let mut ca = rustls::RootCertStore::empty();
    let (added, ignored) = ca.add_parsable_certificates(native_roots);
    debug!(added, ignored, "loaded native root certificate store");
    ca.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let nats = connect(nats)
        .await
        .context("failed to connect to NATS.io")?;
    let nats = wrpc_transport_nats::Client::new(nats, prefix, None);
    let client = hyper_util::client::legacy::Client::builder(TokioExecutor::new()).build(
        hyper_rustls::HttpsConnectorBuilder::new()
            .with_tls_config(
                rustls::ClientConfig::builder()
                    .with_root_certificates(ca)
                    .with_no_client_auth(),
            )
            .https_or_http()
            .enable_all_versions()
            .build(),
    );
    outgoing_handler::serve_interface(&nats, ServeHttp(Handler(client)), async {
        signal::ctrl_c().await.expect("failed to listen for ^C")
    })
    .await
    .context("failed to serve `wrpc:http/outgoing-handler` interface")
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
