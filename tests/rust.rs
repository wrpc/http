use core::pin::pin;
use core::str;
use core::time::Duration;

use std::net::Ipv4Addr;
use std::sync::Arc;

use anyhow::{anyhow, Context};
use futures::{stream, StreamExt as _, TryStreamExt as _};
use hyper::header::HOST;
use hyper::Uri;
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::net::TcpListener;
use tokio::try_join;
use tracing::info;
use wrpc_interface_http::{ErrorCode, Method, Request, RequestOptions, Response, Scheme};
use wrpc_transport::{AcceptedInvocation, Transmitter as _};

mod common;
use common::{init, start_nats};

#[tokio::test(flavor = "multi_thread")]
async fn rust() -> anyhow::Result<()> {
    init().await;

    let (_port, nats_client, nats_server, stop_tx) = start_nats().await?;

    let client = wrpc_transport_nats::Client::new(nats_client, "test-prefix".to_string());
    let client = Arc::new(client);

    {
        use wrpc_interface_http::IncomingHandler;

        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .context("failed to start TCP listener")?;
        let addr = listener
            .local_addr()
            .context("failed to query listener local address")?;

        let invocations = client.serve_handle().await.context("failed to serve")?;
        let mut invocations = pin!(invocations);
        try_join!(
            async {
                let AcceptedInvocation {
                    params:
                        Request {
                            mut body,
                            trailers,
                            method,
                            path_with_query,
                            scheme,
                            authority,
                            headers,
                        },
                    result_subject,
                    transmitter,
                    ..
                } = invocations
                    .try_next()
                    .await
                    .context("failed to receive invocation")?
                    .context("unexpected end of stream")?;
                assert_eq!(method, Method::Post);
                assert_eq!(path_with_query.as_deref(), Some("path_with_query"));
                assert_eq!(scheme, Some(Scheme::HTTPS));
                assert_eq!(authority.as_deref(), Some("authority"));
                assert_eq!(
                    headers,
                    vec![("user-agent".into(), vec!["wrpc/0.1.0".into()])],
                );
                try_join!(
                    async {
                        info!("transmit response");
                        transmitter
                            .transmit_static(
                                result_subject,
                                Ok::<_, ErrorCode>(Response {
                                    body: stream::empty(),
                                    trailers: async { None },
                                    status: 400,
                                    headers: Vec::default(),
                                }),
                            )
                            .await
                            .context("failed to transmit response")?;
                        info!("response transmitted");
                        anyhow::Ok(())
                    },
                    async {
                        let item = body
                            .try_next()
                            .await
                            .context("failed to receive body item")?
                            .context("unexpected end of body stream")?;
                        assert_eq!(str::from_utf8(&item).unwrap(), "element");
                        info!("await request body end");
                        let item = body
                            .try_next()
                            .await
                            .context("failed to receive end item")?;
                        assert_eq!(item, None);
                        info!("request body verified");
                        Ok(())
                    },
                    async {
                        info!("await request trailers");
                        let trailers = trailers.await.context("failed to receive trailers")?;
                        assert_eq!(
                            trailers,
                            Some(vec![("trailer".into(), vec!["test".into()])])
                        );
                        info!("request trailers verified");
                        Ok(())
                    }
                )?;
                let AcceptedInvocation {
                    params:
                        Request {
                            mut body,
                            trailers,
                            method,
                            path_with_query,
                            scheme,
                            authority,
                            mut headers,
                        },
                    result_subject,
                    transmitter,
                    ..
                } = invocations
                    .try_next()
                    .await
                    .context("failed to receive invocation")?
                    .context("unexpected end of stream")?;
                assert_eq!(method, Method::Get);
                assert_eq!(path_with_query.as_deref(), Some("/reqwest"));
                assert_eq!(scheme, Some(Scheme::HTTP));
                assert_eq!(authority, Some(format!("localhost:{}", addr.port())));
                headers.sort();
                assert_eq!(
                    headers,
                    vec![
                        ("accept".into(), vec!["*/*".into()]),
                        (
                            "host".into(),
                            vec![format!("localhost:{}", addr.port()).into()]
                        )
                    ],
                );
                try_join!(
                    async {
                        info!("transmit response");
                        transmitter
                            .transmit_static(
                                result_subject,
                                Ok::<_, ErrorCode>(Response {
                                    body: stream::empty(),
                                    trailers: async { None },
                                    status: 400,
                                    headers: Vec::default(),
                                }),
                            )
                            .await
                            .context("failed to transmit response")?;
                        info!("response transmitted");
                        anyhow::Ok(())
                    },
                    async {
                        let item = body
                            .try_next()
                            .await
                            .context("failed to receive body item")?;
                        assert_eq!(item, None);
                        info!("request body verified");
                        Ok(())
                    },
                    async {
                        info!("await request trailers");
                        let trailers = trailers.await.context("failed to receive trailers")?;
                        assert_eq!(trailers, None);
                        info!("request trailers verified");
                        Ok(())
                    }
                )?;
                anyhow::Ok(())
            },
            async {
                let client = Arc::clone(&client);
                info!("invoke function");
                let (res, tx) = client
                    .invoke_handle(Request {
                        body: stream::iter([("element".into())]),
                        trailers: async { Some(vec![("trailer".into(), vec!["test".into()])]) },
                        method: Method::Post,
                        path_with_query: Some("path_with_query".to_string()),
                        scheme: Some(Scheme::HTTPS),
                        authority: Some("authority".to_string()),
                        headers: vec![("user-agent".into(), vec!["wrpc/0.1.0".into()])],
                    })
                    .await
                    .context("failed to invoke")?;
                let Response {
                    mut body,
                    trailers,
                    status,
                    headers,
                } = res.expect("invocation failed");
                assert_eq!(status, 400);
                assert_eq!(headers, Vec::default());
                try_join!(
                    async {
                        info!("transmit async parameters");
                        tx.await.context("failed to transmit parameters")?;
                        info!("async parameters transmitted");
                        anyhow::Ok(())
                    },
                    async {
                        info!("await response body end");
                        let item = body
                            .try_next()
                            .await
                            .context("failed to receive end item")?;
                        assert_eq!(item, None);
                        info!("response body verified");
                        Ok(())
                    },
                    async {
                        info!("await response trailers");
                        let trailers = trailers.await.context("failed to receive trailers")?;
                        assert_eq!(trailers, None);
                        info!("response trailers verified");
                        Ok(())
                    }
                )?;

                try_join!(
                    async {
                        info!("await connection");
                        let (stream, addr) = listener
                            .accept()
                            .await
                            .context("failed to accept connection")?;
                        info!("accepted connection from {addr}");
                        hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                            .serve_connection(
                                TokioIo::new(stream),
                                hyper::service::service_fn(
                                    move |mut request: hyper::Request<hyper::body::Incoming>| {
                                        let host = request.headers().get(HOST).expect("`host` header missing");
                                        let host = host.to_str().expect("`host` header value is not a valid string");
                                        let path_and_query = request.uri().path_and_query().expect("`path_and_query` missing");
                                        let uri = Uri::builder()
                                            .scheme("http")
                                            .authority(host)
                                            .path_and_query(path_and_query.clone())
                                            .build()
                                            .expect("failed to build a URI");
                                        *request.uri_mut() = uri;
                                        let client = Arc::clone(&client);
                                        async move {
                                            info!(?request, "invoke `handle`");
                                            let (response, tx, errors) =
                                                client.invoke_handle_hyper(request).await.context(
                                                    "failed to invoke `wrpc:http/incoming-handler.handle`",
                                                )?;
                                            info!("await parameter transmit");
                                            tx.await.context("failed to transmit parameters")?;
                                            info!("await error collect");
                                            let errors: Vec<_> = errors.collect().await;
                                            assert!(errors.is_empty());
                                            info!("request served");
                                            response
                                        }
                                    },
                                ),
                            )
                            .await
                            .map_err(|err| anyhow!(err).context("failed to serve connection"))
                    },
                    async {
                        reqwest::get(format!("http://localhost:{}/reqwest", addr.port()))
                            .await
                            .with_context(|| format!("failed to GET `{addr}`"))
                    }
                )?;
                Ok(())
            }
        )?;
    }

    {
        use wrpc_interface_http::OutgoingHandler;

        let invocations = client.serve_handle().await.context("failed to serve")?;
        try_join!(
            async {
                let AcceptedInvocation {
                    params:
                        (
                            Request {
                                mut body,
                                trailers,
                                method,
                                path_with_query,
                                scheme,
                                authority,
                                headers,
                            },
                            opts,
                        ),
                    result_subject,
                    transmitter,
                    ..
                } = pin!(invocations)
                    .try_next()
                    .await
                    .context("failed to receive invocation")?
                    .context("unexpected end of stream")?;
                assert_eq!(method, Method::Get);
                assert_eq!(path_with_query.as_deref(), Some("path_with_query"));
                assert_eq!(scheme, Some(Scheme::HTTPS));
                assert_eq!(authority.as_deref(), Some("authority"));
                assert_eq!(
                    headers,
                    vec![("user-agent".into(), vec!["wrpc/0.1.0".into()])],
                );
                assert_eq!(
                    opts,
                    Some(RequestOptions {
                        connect_timeout: None,
                        first_byte_timeout: Some(Duration::from_nanos(42)),
                        between_bytes_timeout: None,
                    })
                );
                try_join!(
                    async {
                        info!("transmit response");
                        transmitter
                            .transmit_static(
                                result_subject,
                                Ok::<_, ErrorCode>(Response {
                                    body: stream::empty(),
                                    trailers: async { None },
                                    status: 400,
                                    headers: Vec::default(),
                                }),
                            )
                            .await
                            .context("failed to transmit response")?;
                        info!("response transmitted");
                        anyhow::Ok(())
                    },
                    async {
                        info!("await request body element");
                        let item = body
                            .try_next()
                            .await
                            .context("failed to receive body item")?
                            .context("unexpected end of body stream")?;
                        assert_eq!(str::from_utf8(&item).unwrap(), "element");
                        info!("await request body end");
                        let item = body
                            .try_next()
                            .await
                            .context("failed to receive end item")?;
                        assert_eq!(item, None);
                        info!("request body verified");
                        Ok(())
                    },
                    async {
                        info!("await request trailers");
                        let trailers = trailers.await.context("failed to receive trailers")?;
                        assert_eq!(
                            trailers,
                            Some(vec![("trailer".into(), vec!["test".into()])])
                        );
                        info!("request trailers verified");
                        Ok(())
                    }
                )?;
                anyhow::Ok(())
            },
            async {
                info!("invoke function");
                let (res, tx) = client
                    .invoke_handle(
                        Request {
                            body: stream::iter([("element".into())]),
                            trailers: async { Some(vec![("trailer".into(), vec!["test".into()])]) },
                            method: Method::Get,
                            path_with_query: Some("path_with_query".to_string()),
                            scheme: Some(Scheme::HTTPS),
                            authority: Some("authority".to_string()),
                            headers: vec![("user-agent".into(), vec!["wrpc/0.1.0".into()])],
                        },
                        Some(RequestOptions {
                            connect_timeout: None,
                            first_byte_timeout: Some(Duration::from_nanos(42)),
                            between_bytes_timeout: None,
                        }),
                    )
                    .await
                    .context("failed to invoke")?;
                let Response {
                    mut body,
                    trailers,
                    status,
                    headers,
                } = res.expect("invocation failed");
                assert_eq!(status, 400);
                assert_eq!(headers, Vec::default());
                try_join!(
                    async {
                        info!("transmit async parameters");
                        tx.await.context("failed to transmit parameters")?;
                        info!("async parameters transmitted");
                        anyhow::Ok(())
                    },
                    async {
                        info!("await response body end");
                        let item = body
                            .try_next()
                            .await
                            .context("failed to receive end item")?;
                        assert_eq!(item, None);
                        info!("response body verified");
                        Ok(())
                    },
                    async {
                        info!("await response trailers");
                        let trailers = trailers.await.context("failed to receive trailers")?;
                        assert_eq!(trailers, None);
                        info!("response trailers verified");
                        Ok(())
                    }
                )?;
                Ok(())
            }
        )?;
    }

    stop_tx.send(()).expect("failed to stop NATS.io server");
    nats_server
        .await
        .context("failed to await NATS.io server stop")?
        .context("NATS.io server failed to stop")?;
    Ok(())
}
