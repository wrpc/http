use core::iter;
use core::net::{Ipv6Addr, SocketAddr};

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use anyhow::{anyhow, bail, ensure, Context as _};
use bytes::{Buf, Bytes};
use clap::Parser as _;
use criterion::measurement::Measurement;
use criterion::{BenchmarkGroup, Criterion};
use futures::future::try_join_all;
use futures::StreamExt as _;
use http_body_util::{BodyExt as _, Empty, Full};
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::net::{TcpListener, TcpSocket};
use tokio::sync::oneshot;
use tokio::{select, try_join};
use tracing::{debug, instrument, warn};
use wasmtime::component::{Component, Linker, Resource};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiView};
use wasmtime_wasi_http::bindings::ProxyPre;
use wasmtime_wasi_http::body::HostIncomingBody;
use wasmtime_wasi_http::types::HostIncomingRequest;
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpView};
use wrpc_interface_http::{
    HttpBody, InvokeIncomingHandler as _, ServeHttp, ServeIncomingHandlerWasmtime, ServeWasmtime,
};

#[derive(Clone)]
struct NativeHandler;

impl<T: Send> wrpc_interface_http::ServeIncomingHandlerHttp<T> for NativeHandler {
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
            wrpc_interface_http::bindings::wrpc::http::types::ErrorCode,
        >,
    > {
        Ok(Ok(hyper::Response::new(Full::new(Bytes::from(
            "Hello, WASI!",
        )))))
    }
}

struct Ctx {
    table: ResourceTable,
    wasi: WasiCtx,
    http: WasiHttpCtx,
}

impl WasiView for Ctx {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi
    }

    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

impl WasiHttpView for Ctx {
    fn ctx(&mut self) -> &mut WasiHttpCtx {
        &mut self.http
    }

    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

#[derive(Clone)]
struct WasmHandler {
    engine: wasmtime::Engine,
    pre: ProxyPre<Ctx>,
}

impl WasmHandler {
    // https://github.com/bytecodealliance/wasmtime/blob/b943666650696f1eb7ff8b217762b58d5ef5779d/src/commands/serve.rs#L641-L656
    fn use_pooling_allocator_by_default() -> anyhow::Result<Option<bool>> {
        const BITS_TO_TEST: u32 = 42;
        let mut config = wasmtime::Config::new();
        config.wasm_memory64(true);
        config.static_memory_maximum_size(1 << BITS_TO_TEST);
        let engine = wasmtime::Engine::new(&config)?;
        let mut store = wasmtime::Store::new(&engine, ());
        // NB: the maximum size is in wasm pages to take out the 16-bits of wasm
        // page size here from the maximum size.
        let ty = wasmtime::MemoryType::new64(0, Some(1 << (BITS_TO_TEST - 16)));
        if wasmtime::Memory::new(&mut store, ty).is_ok() {
            Ok(Some(true))
        } else {
            Ok(None)
        }
    }

    fn new_store(&self) -> wasmtime::Store<Ctx> {
        wasmtime::Store::new(
            &self.engine,
            Ctx {
                table: ResourceTable::new(),
                wasi: WasiCtxBuilder::new()
                    .inherit_env()
                    .inherit_stdio()
                    .inherit_network()
                    .allow_ip_name_lookup(true)
                    .allow_tcp(true)
                    .allow_udp(true)
                    .build(),
                http: WasiHttpCtx::new(),
            },
        )
    }

    async fn call_handle(
        &self,
        mut store: wasmtime::Store<Ctx>,
        request: Resource<HostIncomingRequest>,
    ) -> anyhow::Result<
        Result<
            http::Response<wasmtime_wasi_http::body::HyperOutgoingBody>,
            wasmtime_wasi_http::bindings::http::types::ErrorCode,
        >,
    > {
        let (tx, rx) = oneshot::channel();
        let data = store.data_mut();

        let response = data
            .new_response_outparam(tx)
            .context("failed to create response")?;
        let proxy = self
            .pre
            .instantiate_async(&mut store)
            .await
            .context("failed to instantiate `wasi:http/incoming-handler`")?;
        let handle = tokio::spawn(async move {
            debug!("invoking `wasi:http/incoming-handler.handle`");
            if let Err(err) = proxy
                .wasi_http_incoming_handler()
                .call_handle(&mut store, request, response)
                .await
            {
                warn!(?err, "failed to call `wasi:http/incoming-handler.handle`");
                bail!(err.context("failed to call `wasi:http/incoming-handler.handle`"));
            }
            Ok(())
        });
        debug!("awaiting `wasi:http/incoming-handler.handle` response");
        match rx.await {
            Ok(Ok(res)) => {
                debug!("successful `wasi:http/incoming-handler.handle` response received");
                Ok(Ok(res))
            }
            Ok(Err(err)) => {
                debug!(
                    ?err,
                    "unsuccessful `wasi:http/incoming-handler.handle` response received"
                );
                Ok(Err(err))
            }
            Err(_) => {
                debug!("`wasi:http/incoming-handler.handle` response sender dropped");
                handle.await.context("failed to join handle task")??;
                bail!("component did not call `response-outparam::set`")
            }
        }
    }

    pub async fn new(wasm: &[u8]) -> anyhow::Result<Self> {
        let mut opts =
            wasmtime_cli_flags::CommonOptions::try_parse_from(iter::empty::<&'static str>())
                .context("failed to construct common Wasmtime options")?;
        let mut config = opts
            .config(
                None,
                Self::use_pooling_allocator_by_default().unwrap_or(None),
            )
            .context("failed to construct Wasmtime config")?;
        config.wasm_component_model(true);
        config.async_support(true);
        let engine =
            wasmtime::Engine::new(&config).context("failed to initialize Wasmtime engine")?;

        let component = Component::new(&engine, wasm).context("failed to compile component")?;

        let mut linker = Linker::<Ctx>::new(&engine);
        wasmtime_wasi::add_to_linker_async(&mut linker).context("failed to link WASI")?;
        wasmtime_wasi_http::add_only_http_to_linker_async(&mut linker)
            .context("failed to link `wasi:http`")?;

        let pre = linker
            .instantiate_pre(&component)
            .context("failed to pre-instantiate component")?;
        let pre = ProxyPre::new(pre).context("failed to pre-instantiate `proxy` world")?;
        Ok(Self { engine, pre })
    }
}

impl<T: Send> ServeIncomingHandlerWasmtime<T> for WasmHandler {
    #[instrument(level = "trace", skip_all)]
    async fn handle(
        &self,
        _cx: T,
        request: http::Request<wasmtime_wasi_http::body::HyperIncomingBody>,
    ) -> anyhow::Result<
        Result<
            http::Response<wasmtime_wasi_http::body::HyperOutgoingBody>,
            wasmtime_wasi_http::bindings::http::types::ErrorCode,
        >,
    > {
        use wasmtime_wasi_http::bindings::http::types::Scheme;

        let scheme: Scheme = request
            .uri()
            .scheme()
            .map(wrpc_interface_http::bindings::wrpc::http::types::Scheme::from)
            .map(Into::into)
            .unwrap_or(Scheme::Http);

        let mut store = self.new_store();
        let data = store.data_mut();

        // The below is adapted from `WasiHttpView::new_incoming_request`, which is unusable for
        // us, since it requires a `hyper::Error`

        let (parts, body) = request.into_parts();
        let body = HostIncomingBody::new(
            body,
            // TODO: this needs to be plumbed through
            std::time::Duration::from_millis(600 * 1000),
        );
        let incoming_req = HostIncomingRequest::new(data, parts, scheme, Some(body))?;
        let request = WasiHttpView::table(data).push(incoming_req)?;

        self.call_handle(store, request).await
    }
}

pub fn with_nats<T>(
    rt: &tokio::runtime::Runtime,
    f: impl FnOnce(async_nats::Client) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let (_, nats_clt, nats_srv, stop_tx) = rt.block_on(wrpc_test::start_nats())?;
    let res = f(nats_clt).context("closure failed")?;
    stop_tx.send(()).expect("failed to stop NATS.io server");
    rt.block_on(nats_srv)
        .context("failed to await NATS.io server stop")?
        .context("NATS.io server failed to stop")?;
    Ok(res)
}

async fn new_socket() -> std::io::Result<(TcpListener, SocketAddr)> {
    let socket = TcpSocket::new_v6()?;
    socket.set_reuseaddr(!cfg!(windows))?;
    socket.bind((Ipv6Addr::LOCALHOST, 0).into())?;
    let socket = socket.listen(128)?;
    let addr = socket.local_addr()?;
    Ok((socket, addr))
}

async fn serve_connections<S, B>(rt: tokio::runtime::Handle, socket: TcpListener, svc: S)
where
    S: hyper::service::Service<http::Request<hyper::body::Incoming>, Response = http::Response<B>>
        + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
    S::Error: Into<Box<dyn core::error::Error + Send + Sync>>,
    B: http_body::Body + Send + 'static,
    B::Data: Send + 'static,
    B::Error: Into<Box<dyn core::error::Error + Send + Sync>>,
{
    let mut srv = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new());
    srv.http1().keep_alive(false);
    let srv = Arc::new(srv);
    loop {
        let (stream, _) = socket.accept().await.expect("failed to accept");
        rt.spawn({
            let srv = Arc::clone(&srv);
            let svc = svc.clone();
            async move {
                srv.serve_connection(TokioIo::new(stream), svc)
                    .await
                    .expect("failed to serve connection");
            }
        });
    }
}

async fn get_bytes(
    addr: SocketAddr,
    req: http::Request<Empty<impl Buf + Send + 'static>>,
) -> anyhow::Result<Bytes> {
    let stream = loop {
        let socket = match &addr {
            SocketAddr::V4(_) => TcpSocket::new_v4()?,
            SocketAddr::V6(_) => TcpSocket::new_v6()?,
        };
        match socket.connect(addr).await {
            Ok(stream) => break stream,
            Err(err) if err.kind() != std::io::ErrorKind::TimedOut => {
                bail!(anyhow!(err).context("failed to connect"))
            }
            Err(_) => continue,
        }
    };
    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .context("failed to complete handshake")?;
    let ((), body) = try_join!(async { conn.await.context("connection failed") }, async {
        let res = sender
            .send_request(req)
            .await
            .context("failed to send request")?;
        ensure!(res.status().is_success());
        res.into_body()
            .collect()
            .await
            .context("failed to receive body")
    })?;
    Ok(body.to_bytes())
}

fn bench_hello(
    g: &mut BenchmarkGroup<impl Measurement>,
    id: &str,
    rt: &tokio::runtime::Runtime,
    addr: SocketAddr,
) -> anyhow::Result<()> {
    let req = http::Request::builder()
        .uri(format!("http://{addr}/test"))
        .header(http::header::HOST, "localhost")
        .body(Empty::<Bytes>::new())
        .context("failed to construct request")?;
    g.bench_function(format!("{id}/1"), |b| {
        b.to_async(rt).iter(|| async {
            let buf = get_bytes(addr, req.clone())
                .await
                .expect("GET request failed");
            assert_eq!(buf, "Hello, WASI!");
        })
    });
    g.bench_with_input(format!("{id}/50"), &req, |b, req| {
        b.to_async(rt).iter(|| async {
            let mut tasks = Vec::with_capacity(1000);
            for _ in 0..50 {
                let req = req.clone();
                tasks.push(rt.spawn(async move {
                    let buf = get_bytes(addr, req).await.expect("GET request failed");
                    assert_eq!(buf, "Hello, WASI!");
                }));
            }
            try_join_all(tasks).await.expect("GET request failed")
        })
    });
    Ok(())
}

fn bench_wasm_direct(g: &mut BenchmarkGroup<impl Measurement>, wasm: &[u8]) -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new().context("failed to build Tokio runtime")?;
    let handler = rt
        .block_on(WasmHandler::new(wasm))
        .context("failed to construct a Wasm handler")?;
    let (socket, addr) = rt.block_on(new_socket())?;
    let accept = rt.spawn(serve_connections(
        rt.handle().clone(),
        socket,
        hyper::service::service_fn(move |request| {
            let handler = handler.clone();
            async move {
                let mut store = handler.new_store();
                let request = store
                    .data_mut()
                    .new_incoming_request(
                        wasmtime_wasi_http::bindings::http::types::Scheme::Http,
                        request,
                    )
                    .expect("failed to push new request");
                handler
                    .call_handle(store, request)
                    .await
                    .expect("failed to call `handle`")
            }
        }),
    ));
    bench_hello(g, "direct Wasm", &rt, addr)?;
    accept.abort();
    Ok(())
}

fn bench_wasm_nats(g: &mut BenchmarkGroup<impl Measurement>, wasm: &[u8]) -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new().context("failed to build Tokio runtime")?;
    let handler = rt
        .block_on(WasmHandler::new(wasm))
        .context("failed to construct a Wasm handler")?;
    let (socket, addr) = rt.block_on(new_socket())?;
    with_nats(&rt, |nats| {
        let handle = rt.handle().clone();
        let wrpc = rt
            .block_on(wrpc_transport_nats::Client::new(nats, "", None))
            .context("failed to construct client")?;

        let [(_, _, mut invocations)] = rt
            .block_on(
                wrpc_interface_http::bindings::exports::wrpc::http::incoming_handler::serve_interface(
                    &wrpc,
                    ServeWasmtime(handler),
                ),
            )
            .context("failed to serve bindings")?;
        let (stop_tx, mut stop_rx) = oneshot::channel();
        let srv = handle.spawn({
            let handle = handle.clone();
            async move {
                loop {
                    select! {
                        Some(res) = invocations.next() => {
                            let fut = res.expect("failed to accept invocation");
                            handle.spawn(fut);
                        },
                        _ = &mut stop_rx => {
                            drop(invocations);
                            return anyhow::Ok(())
                        },
                    }
                }
            }
        });
        let accept = rt.spawn(serve_connections(
            handle.clone(),
            socket,
            hyper::service::service_fn({
                move |request| {
                    let handle = handle.clone();
                    let wrpc = wrpc.clone();
                    async move {
                        let (res, errs, io) = wrpc
                            .invoke_handle_http(None, request)
                            .await
                            .expect("failed to invoke handler");
                        if let Some(io) = io {
                            handle.spawn(
                                async move { io.await.expect("failed to handle async I/O") },
                            );
                        }
                        handle.spawn(errs.for_each(|err| async move {
                            panic!("body error encountered: {err:?}")
                        }));
                        res
                    }
                }
            }),
        ));
        bench_hello(g, "wRPC NATS.io Wasm", &rt, addr)?;
        accept.abort();
        stop_tx.send(()).expect("failed to stop server");
        rt.block_on(async { srv.await.context("server task panicked")? })?;
        Ok(())
    })
}

fn bench_native_nats(g: &mut BenchmarkGroup<impl Measurement>) -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new().context("failed to build Tokio runtime")?;
    let (socket, addr) = rt.block_on(new_socket())?;
    with_nats(&rt, |nats| {
        let handle = rt.handle().clone();
        let wrpc = rt
            .block_on(wrpc_transport_nats::Client::new(nats, "", None))
            .context("failed to construct client")?;

        let [(_, _, mut invocations)] = rt
            .block_on(
                wrpc_interface_http::bindings::exports::wrpc::http::incoming_handler::serve_interface(
                    &wrpc,
                    ServeHttp(NativeHandler),
                ),
            )
            .context("failed to serve bindings")?;
        let (stop_tx, mut stop_rx) = oneshot::channel();
        let srv = handle.spawn({
            let handle = handle.clone();
            async move {
                loop {
                    select! {
                        Some(res) = invocations.next() => {
                            let fut = res.expect("failed to accept invocation");
                            handle.spawn(fut);
                        },
                        _ = &mut stop_rx => {
                            drop(invocations);
                            return anyhow::Ok(())
                        },
                    }
                }
            }
        });
        let accept = rt.spawn(serve_connections(
            handle.clone(),
            socket,
            hyper::service::service_fn({
                move |request| {
                    let handle = handle.clone();
                    let wrpc = wrpc.clone();
                    async move {
                        let (res, errs, io) = wrpc
                            .invoke_handle_http(None, request)
                            .await
                            .expect("failed to invoke handler");
                        if let Some(io) = io {
                            handle.spawn(
                                async move { io.await.expect("failed to handle async I/O") },
                            );
                        }
                        handle.spawn(errs.for_each(|err| async move {
                            panic!("body error encountered: {err:?}")
                        }));
                        res
                    }
                }
            }),
        ));
        bench_hello(g, "wRPC NATS.io native", &rt, addr)?;
        accept.abort();
        stop_tx.send(()).expect("failed to stop server");
        rt.block_on(async { srv.await.context("server task panicked")? })?;
        Ok(())
    })
}

fn main() -> anyhow::Result<()> {
    let res = Command::new(env!("CARGO"))
        .args([
            "build",
            "--release",
            "--target",
            "wasm32-wasip2",
            "--manifest-path",
        ])
        .arg(PathBuf::from_iter([
            env!("CARGO_MANIFEST_DIR"),
            "benches",
            "incoming",
            "Cargo.toml",
        ]))
        .status()
        .context("failed to build `reactor.wasm`")?;
    assert!(res.success());
    let reactor = fs::read(PathBuf::from_iter([
        env!("CARGO_MANIFEST_DIR"),
        "benches",
        "incoming",
        "target",
        "wasm32-wasip2",
        "release",
        "incoming.wasm",
    ]))
    .context("failed to read `incoming.wasm`")?;
    let mut c = Criterion::default().configure_from_args();
    {
        let mut g = c.benchmark_group("incoming");
        bench_wasm_direct(&mut g, &reactor)?;
        bench_wasm_nats(&mut g, &reactor)?;
        bench_native_nats(&mut g)?;
        g.finish();
    }
    c.final_summary();
    Ok(())
}
