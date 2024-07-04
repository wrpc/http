use std::sync::Arc;

use anyhow::Context;
use async_nats::HeaderMap;
use bytes::Bytes;
use futures::StreamExt as _;
use http_body_util::BodyExt as _;
use hyper::Uri;
use tokio::sync::Notify;
use tracing::info;
use wrpc_interface_http::HttpBody;
use wrpc_interface_http::{InvokeIncomingHandler as _, ServeHttp};

mod common;
use common::{init, start_nats};

#[tokio::test(flavor = "multi_thread")]
async fn rust() -> anyhow::Result<()> {
    init().await;

    let (_port, nats_client, nats_server, stop_tx) = start_nats().await?;

    let client = wrpc_transport_nats::Client::new(nats_client, "test-prefix".to_string(), None);
    let client = Arc::new(client);

    {
        #[derive(Clone)]
        struct Handler;

        impl wrpc_interface_http::ServeIncomingHandlerHttp<Option<HeaderMap>> for Handler {
            async fn handle(
                &self,
                cx: Option<HeaderMap>,
                req: hyper::Request<HttpBody>,
            ) -> anyhow::Result<
                Result<
                    hyper::Response<
                        impl http_body::Body<Data = Bytes, Error = core::convert::Infallible>
                            + Send
                            + Sync
                            + 'static,
                    >,
                    wrpc_interface_http::bindings::wrpc::http::types::ErrorCode,
                >,
            > {
                assert_eq!(cx, None);
                let (
                    http::request::Parts {
                        method,
                        uri,
                        headers,
                        ..
                    },
                    body,
                ) = req.into_parts();
                let mut headers = headers
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.to_str().unwrap()))
                    .collect::<Vec<_>>();
                headers.sort_unstable();
                assert_eq!(method, http::Method::POST);
                assert_eq!(uri.to_string(), "https://example.com/test?foo=bar");
                assert_eq!(headers, [("user-agent", "wrpc/0.2.0")],);
                let mut res = hyper::Response::new(body.map_err(|err| {
                    panic!("body error encountered: {err}");
                }));
                res.headers_mut().insert(
                    http::HeaderName::from_static("foo"),
                    http::HeaderValue::from_static("bar"),
                );
                Ok(Ok(res))
            }
        }

        let shutdown = Arc::new(Notify::new());

        info!("serving incoming handler");
        let server = tokio::spawn({
            let client = Arc::clone(&client);
            let shutdown = Arc::clone(&shutdown);
            async move {
                wrpc_interface_http::bindings::exports::wrpc::http::incoming_handler::serve_interface(
                client.as_ref(),
                ServeHttp(Handler),
                {
                    async move { shutdown.notified().await }
                },
            )
            .await
            .context("failed to serve incoming handler")
            }
        });

        let mut req = hyper::Request::new(http_body_util::Full::new(Bytes::from("foobar")));
        *req.method_mut() = http::Method::POST;
        *req.uri_mut() = Uri::from_static("https://example.com/test?foo=bar");
        req.headers_mut().insert(
            http::HeaderName::from_static("user-agent"),
            http::HeaderValue::from_static("wrpc/0.2.0"),
        );
        info!("invoking incoming handler");
        let (res, err, io) = client.invoke_handle_http(None, req).await?;
        let res = res?;
        if let Some(io) = io {
            io.await?;
        }
        let err = err.collect::<Vec<_>>().await;
        assert!(matches!(err[..], []));
        let (
            http::response::Parts {
                status, headers, ..
            },
            body,
        ) = res.into_parts();
        let mut headers = headers
            .iter()
            .map(|(k, v)| (k.as_str(), v.to_str().unwrap()))
            .collect::<Vec<_>>();
        headers.sort_unstable();
        assert_eq!(status, http::StatusCode::OK);
        assert_eq!(headers, [("foo", "bar")]);
        let body = body.collect().await?;
        assert_eq!(body.to_bytes(), "foobar");

        shutdown.notify_one();
        server.await??;
    };

    stop_tx.send(()).expect("failed to stop NATS.io server");
    nats_server
        .await
        .context("failed to await NATS.io server stop")?
        .context("NATS.io server failed to stop")?;
    Ok(())
}
