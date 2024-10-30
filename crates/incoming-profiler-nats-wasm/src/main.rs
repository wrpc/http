use core::pin::pin;

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{anyhow, bail, Context as _};
use clap::Parser;
use futures::StreamExt as _;
use http::uri::Uri;
use hyper_util::rt::{TokioExecutor, TokioIo};
use pprof::protos::Message as _;
use tokio::sync::oneshot;
use tokio::{fs, select, signal};
use tracing::{debug, error, instrument, warn};
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use url::Url;
use wasi_preview1_component_adapter_provider::{
    WASI_SNAPSHOT_PREVIEW1_ADAPTER_NAME, WASI_SNAPSHOT_PREVIEW1_REACTOR_ADAPTER,
};
use wasmtime::component::{Component, Linker, Resource};
use wasmtime::{Engine, Store};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiView};
use wasmtime_wasi_http::bindings::http::types::{ErrorCode, Scheme};
use wasmtime_wasi_http::bindings::ProxyPre;
use wasmtime_wasi_http::body::HostIncomingBody;
use wasmtime_wasi_http::types::HostIncomingRequest;
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpView};
use wrpc_interface_http::bindings::exports::wrpc::http::incoming_handler;
use wrpc_interface_http::{
    InvokeIncomingHandler as _, ServeIncomingHandlerWasmtime, ServeWasmtime,
};

// https://github.com/bytecodealliance/wasmtime/blob/b943666650696f1eb7ff8b217762b58d5ef5779d/src/commands/serve.rs#L641-L656
fn use_pooling_allocator_by_default() -> anyhow::Result<Option<bool>> {
    const BITS_TO_TEST: u32 = 42;
    let mut config = wasmtime::Config::new();
    config.wasm_memory64(true);
    config.static_memory_maximum_size(1 << BITS_TO_TEST);
    let engine = Engine::new(&config)?;
    let mut store = Store::new(&engine, ());
    // NB: the maximum size is in wasm pages to take out the 16-bits of wasm
    // page size here from the maximum size.
    let ty = wasmtime::MemoryType::new64(0, Some(1 << (BITS_TO_TEST - 16)));
    if wasmtime::Memory::new(&mut store, ty).is_ok() {
        Ok(Some(true))
    } else {
        Ok(None)
    }
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(flatten)]
    common: wasmtime_cli_flags::CommonOptions,

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

    workload: String,
}

pub enum Workload {
    Url(Url),
    Binary(Vec<u8>),
}

#[derive(Clone)]
pub struct Handler {
    engine: Engine,
    pre: ProxyPre<Ctx>,
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

fn new_store(engine: &Engine) -> Store<Ctx> {
    Store::new(
        engine,
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
    mut store: Store<Ctx>,
    pre: &ProxyPre<Ctx>,
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
    let proxy = pre
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

impl ServeIncomingHandlerWasmtime<Option<async_nats::HeaderMap>> for Handler {
    #[instrument(level = "trace", skip_all)]
    async fn handle(
        &self,
        _cx: Option<async_nats::HeaderMap>,
        request: http::Request<wasmtime_wasi_http::body::HyperIncomingBody>,
    ) -> anyhow::Result<
        Result<
            http::Response<wasmtime_wasi_http::body::HyperOutgoingBody>,
            wasmtime_wasi_http::bindings::http::types::ErrorCode,
        >,
    > {
        let scheme: Scheme = request
            .uri()
            .scheme()
            .map(wrpc_interface_http::bindings::wrpc::http::types::Scheme::from)
            .map(Into::into)
            .unwrap_or(Scheme::Http);

        let mut store = new_store(&self.engine);
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

        call_handle(store, &self.pre, request).await
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
        mut common,
        nats,
        prefix,
        addr,
        profile,
        ref workload,
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

    let mut config = common
        .config(None, use_pooling_allocator_by_default().unwrap_or(None))
        .context("failed to construct Wasmtime config")?;
    config.wasm_component_model(true);
    config.async_support(true);
    let engine = Engine::new(&config).context("failed to initialize Wasmtime engine")?;

    let wasm = if workload.starts_with('.') || workload.starts_with('/') {
        fs::read(&workload)
            .await
            .with_context(|| format!("failed to read relative path to workload `{workload}`"))
            .map(Workload::Binary)
    } else {
        Url::parse(workload)
            .with_context(|| format!("failed to parse Wasm URL `{workload}`"))
            .map(Workload::Url)
    }?;
    let wasm = match wasm {
        Workload::Url(wasm) => match wasm.scheme() {
            "file" => {
                let wasm = wasm
                    .to_file_path()
                    .map_err(|()| anyhow!("failed to convert Wasm URL to file path"))?;
                fs::read(wasm)
                    .await
                    .context("failed to read Wasm from file URL")?
            }
            "http" | "https" => {
                let wasm = reqwest::get(wasm).await.context("failed to GET Wasm URL")?;
                let wasm = wasm.bytes().await.context("failed fetch Wasm from URL")?;
                wasm.to_vec()
            }
            scheme => bail!("URL scheme `{scheme}` not supported"),
        },
        Workload::Binary(wasm) => wasm,
    };
    let wasm = if wasmparser::Parser::is_core_wasm(&wasm) {
        wit_component::ComponentEncoder::default()
            .validate(true)
            .module(&wasm)
            .context("failed to set core component module")?
            .adapter(
                WASI_SNAPSHOT_PREVIEW1_ADAPTER_NAME,
                WASI_SNAPSHOT_PREVIEW1_REACTOR_ADAPTER,
            )
            .context("failed to add WASI adapter")?
            .encode()
            .context("failed to encode a component")?
    } else {
        wasm
    };

    let component = Component::new(&engine, wasm).context("failed to compile component")?;

    let mut linker = Linker::<Ctx>::new(&engine);
    wasmtime_wasi::add_to_linker_async(&mut linker).context("failed to link WASI")?;
    wasmtime_wasi_http::add_only_http_to_linker_async(&mut linker)
        .context("failed to link `wasi:http`")?;

    let pre = linker
        .instantiate_pre(&component)
        .context("failed to pre-instantiate component")?;
    let pre = ProxyPre::new(pre).context("failed to pre-instantiate `proxy` world")?;

    let [(_, _, mut invocations)] = incoming_handler::serve_interface(
        &wrpc,
        ServeWasmtime(Handler {
            engine: engine.clone(),
            pre: pre.clone(),
        }),
    )
    .await
    .context("failed to serve `wrpc:http/incoming-handler` interface")?;

    let socket = match &addr {
        SocketAddr::V4(_) => tokio::net::TcpSocket::new_v4()?,
        SocketAddr::V6(_) => tokio::net::TcpSocket::new_v6()?,
    };
    socket.set_reuseaddr(!cfg!(windows))?;
    socket.bind(addr)?;
    let listener = socket.listen(1024)?;

    let svc = hyper::service::service_fn(move |req| {
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
    });

    let srv = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new());
    let srv = Arc::new(srv);

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
                let svc = svc.clone();
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
