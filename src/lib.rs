pub mod bindings {
    wit_bindgen_wrpc::generate!({
        world: "interfaces",
        generate_all,
    });
}

#[cfg(feature = "http")]
pub fn try_fields_to_header_map(
    fields: bindings::wrpc::http::types::Fields,
) -> anyhow::Result<http::HeaderMap> {
    use anyhow::Context as _;

    let mut headers = http::HeaderMap::new();
    for (name, values) in fields {
        let name: http::HeaderName = name.parse().context("failed to parse header name")?;
        let http::header::Entry::Vacant(entry) = headers.entry(name) else {
            anyhow::bail!("duplicate header entry");
        };
        let Some((first, values)) = values.split_first() else {
            continue;
        };
        let first = first
            .as_ref()
            .try_into()
            .context("failed to construct header value")?;
        let mut entry = entry.insert_entry(first);
        for value in values {
            let value = value
                .as_ref()
                .try_into()
                .context("failed to construct header value")?;
            entry.append(value);
        }
    }
    Ok(headers)
}

#[cfg(feature = "http")]
pub fn try_header_map_to_fields(
    headers: http::HeaderMap,
) -> anyhow::Result<bindings::wrpc::http::types::Fields> {
    use anyhow::Context as _;

    let headers_len = headers.keys_len();
    headers
        .into_iter()
        .try_fold(
            Vec::with_capacity(headers_len),
            |mut headers, (name, value)| {
                if let Some(name) = name {
                    headers.push((name.to_string(), vec![value.as_bytes().to_vec().into()]));
                } else {
                    let (_, ref mut values) = headers
                        .last_mut()
                        .context("header name missing and fields are empty")?;
                    values.push(value.as_bytes().to_vec().into());
                }
                anyhow::Ok(headers)
            },
        )
        .context("failed to construct fields")
}

#[cfg(feature = "http")]
impl From<&http::Method> for bindings::wrpc::http::types::Method {
    fn from(method: &http::Method) -> Self {
        match method.as_str() {
            "GET" => Self::Get,
            "HEAD" => Self::Head,
            "POST" => Self::Post,
            "PUT" => Self::Put,
            "DELETE" => Self::Delete,
            "CONNECT" => Self::Connect,
            "OPTIONS" => Self::Options,
            "TRACE" => Self::Trace,
            "PATCH" => Self::Patch,
            _ => Self::Other(method.to_string()),
        }
    }
}

#[cfg(feature = "http")]
impl TryFrom<&bindings::wrpc::http::types::Method> for http::method::Method {
    type Error = http::method::InvalidMethod;

    fn try_from(method: &bindings::wrpc::http::types::Method) -> Result<Self, Self::Error> {
        use bindings::wrpc::http::types::Method;

        match method {
            Method::Get => Ok(Self::GET),
            Method::Head => Ok(Self::HEAD),
            Method::Post => Ok(Self::POST),
            Method::Put => Ok(Self::PUT),
            Method::Delete => Ok(Self::DELETE),
            Method::Connect => Ok(Self::CONNECT),
            Method::Options => Ok(Self::OPTIONS),
            Method::Trace => Ok(Self::TRACE),
            Method::Patch => Ok(Self::PATCH),
            Method::Other(method) => method.parse(),
        }
    }
}

#[cfg(feature = "wasmtime-wasi-http")]
impl From<wasmtime_wasi_http::bindings::http::types::Method>
    for bindings::wrpc::http::types::Method
{
    fn from(method: wasmtime_wasi_http::bindings::http::types::Method) -> Self {
        match method {
            wasmtime_wasi_http::bindings::http::types::Method::Get => Self::Get,
            wasmtime_wasi_http::bindings::http::types::Method::Head => Self::Head,
            wasmtime_wasi_http::bindings::http::types::Method::Post => Self::Post,
            wasmtime_wasi_http::bindings::http::types::Method::Put => Self::Put,
            wasmtime_wasi_http::bindings::http::types::Method::Delete => Self::Delete,
            wasmtime_wasi_http::bindings::http::types::Method::Connect => Self::Connect,
            wasmtime_wasi_http::bindings::http::types::Method::Options => Self::Options,
            wasmtime_wasi_http::bindings::http::types::Method::Trace => Self::Trace,
            wasmtime_wasi_http::bindings::http::types::Method::Patch => Self::Patch,
            wasmtime_wasi_http::bindings::http::types::Method::Other(other) => Self::Other(other),
        }
    }
}

#[cfg(feature = "wasmtime-wasi-http")]
impl From<bindings::wrpc::http::types::Method>
    for wasmtime_wasi_http::bindings::http::types::Method
{
    fn from(method: bindings::wrpc::http::types::Method) -> Self {
        use bindings::wrpc::http::types::Method;

        match method {
            Method::Get => Self::Get,
            Method::Head => Self::Head,
            Method::Post => Self::Post,
            Method::Put => Self::Put,
            Method::Delete => Self::Delete,
            Method::Connect => Self::Connect,
            Method::Options => Self::Options,
            Method::Trace => Self::Trace,
            Method::Patch => Self::Patch,
            Method::Other(other) => Self::Other(other),
        }
    }
}

#[cfg(feature = "http")]
impl From<&http::uri::Scheme> for bindings::wrpc::http::types::Scheme {
    fn from(scheme: &http::uri::Scheme) -> Self {
        match scheme.as_str() {
            "http" => Self::Http,
            "https" => Self::Https,
            _ => Self::Other(scheme.to_string()),
        }
    }
}

#[cfg(feature = "http")]
impl TryFrom<&bindings::wrpc::http::types::Scheme> for http::uri::Scheme {
    type Error = http::uri::InvalidUri;

    fn try_from(scheme: &bindings::wrpc::http::types::Scheme) -> Result<Self, Self::Error> {
        use bindings::wrpc::http::types::Scheme;

        match scheme {
            Scheme::Http => Ok(Self::HTTP),
            Scheme::Https => Ok(Self::HTTPS),
            Scheme::Other(scheme) => scheme.parse(),
        }
    }
}

#[cfg(feature = "wasmtime-wasi-http")]
impl From<wasmtime_wasi_http::bindings::http::types::Scheme>
    for bindings::wrpc::http::types::Scheme
{
    fn from(scheme: wasmtime_wasi_http::bindings::http::types::Scheme) -> Self {
        match scheme {
            wasmtime_wasi_http::bindings::http::types::Scheme::Http => Self::Http,
            wasmtime_wasi_http::bindings::http::types::Scheme::Https => Self::Https,
            wasmtime_wasi_http::bindings::http::types::Scheme::Other(other) => Self::Other(other),
        }
    }
}

#[cfg(feature = "wasmtime-wasi-http")]
impl From<bindings::wrpc::http::types::Scheme>
    for wasmtime_wasi_http::bindings::http::types::Scheme
{
    fn from(scheme: bindings::wrpc::http::types::Scheme) -> Self {
        use bindings::wrpc::http::types::Scheme;

        match scheme {
            Scheme::Http => Self::Http,
            Scheme::Https => Self::Https,
            Scheme::Other(other) => Self::Other(other),
        }
    }
}

#[cfg(feature = "wasmtime-wasi-http")]
impl From<wasmtime_wasi_http::bindings::http::types::DnsErrorPayload>
    for bindings::wasi::http::types::DnsErrorPayload
{
    fn from(
        wasmtime_wasi_http::bindings::http::types::DnsErrorPayload { rcode, info_code }: wasmtime_wasi_http::bindings::http::types::DnsErrorPayload,
    ) -> Self {
        Self { rcode, info_code }
    }
}

#[cfg(feature = "wasmtime-wasi-http")]
impl From<bindings::wasi::http::types::DnsErrorPayload>
    for wasmtime_wasi_http::bindings::http::types::DnsErrorPayload
{
    fn from(
        bindings::wasi::http::types::DnsErrorPayload { rcode, info_code }: bindings::wasi::http::types::DnsErrorPayload,
    ) -> Self {
        Self { rcode, info_code }
    }
}

#[cfg(feature = "wasmtime-wasi-http")]
impl From<wasmtime_wasi_http::bindings::http::types::TlsAlertReceivedPayload>
    for bindings::wasi::http::types::TlsAlertReceivedPayload
{
    fn from(
        wasmtime_wasi_http::bindings::http::types::TlsAlertReceivedPayload {
            alert_id,
            alert_message,
        }: wasmtime_wasi_http::bindings::http::types::TlsAlertReceivedPayload,
    ) -> Self {
        Self {
            alert_id,
            alert_message,
        }
    }
}

#[cfg(feature = "wasmtime-wasi-http")]
impl From<bindings::wasi::http::types::TlsAlertReceivedPayload>
    for wasmtime_wasi_http::bindings::http::types::TlsAlertReceivedPayload
{
    fn from(
        bindings::wasi::http::types::TlsAlertReceivedPayload {
            alert_id,
            alert_message,
        }: bindings::wasi::http::types::TlsAlertReceivedPayload,
    ) -> Self {
        Self {
            alert_id,
            alert_message,
        }
    }
}

#[cfg(feature = "wasmtime-wasi-http")]
impl From<wasmtime_wasi_http::bindings::http::types::FieldSizePayload>
    for bindings::wasi::http::types::FieldSizePayload
{
    fn from(
        wasmtime_wasi_http::bindings::http::types::FieldSizePayload {
            field_name,
            field_size,
        }: wasmtime_wasi_http::bindings::http::types::FieldSizePayload,
    ) -> Self {
        Self {
            field_name,
            field_size,
        }
    }
}

#[cfg(feature = "wasmtime-wasi-http")]
impl From<bindings::wasi::http::types::FieldSizePayload>
    for wasmtime_wasi_http::bindings::http::types::FieldSizePayload
{
    fn from(
        bindings::wasi::http::types::FieldSizePayload {
            field_name,
            field_size,
        }: bindings::wasi::http::types::FieldSizePayload,
    ) -> Self {
        Self {
            field_name,
            field_size,
        }
    }
}

#[cfg(feature = "wasmtime-wasi-http")]
impl From<wasmtime_wasi_http::bindings::http::types::ErrorCode>
    for bindings::wasi::http::types::ErrorCode
{
    fn from(code: wasmtime_wasi_http::bindings::http::types::ErrorCode) -> Self {
        use wasmtime_wasi_http::bindings::http::types;
        match code {
            types::ErrorCode::DnsTimeout => Self::DnsTimeout,
            types::ErrorCode::DnsError(err) => Self::DnsError(err.into()),
            types::ErrorCode::DestinationNotFound => Self::DestinationNotFound,
            types::ErrorCode::DestinationUnavailable => Self::DestinationUnavailable,
            types::ErrorCode::DestinationIpProhibited => Self::DestinationIpProhibited,
            types::ErrorCode::DestinationIpUnroutable => Self::DestinationIpUnroutable,
            types::ErrorCode::ConnectionRefused => Self::ConnectionRefused,
            types::ErrorCode::ConnectionTerminated => Self::ConnectionTerminated,
            types::ErrorCode::ConnectionTimeout => Self::ConnectionTimeout,
            types::ErrorCode::ConnectionReadTimeout => Self::ConnectionReadTimeout,
            types::ErrorCode::ConnectionWriteTimeout => Self::ConnectionWriteTimeout,
            types::ErrorCode::ConnectionLimitReached => Self::ConnectionLimitReached,
            types::ErrorCode::TlsProtocolError => Self::TlsProtocolError,
            types::ErrorCode::TlsCertificateError => Self::TlsCertificateError,
            types::ErrorCode::TlsAlertReceived(err) => Self::TlsAlertReceived(err.into()),
            types::ErrorCode::HttpRequestDenied => Self::HttpRequestDenied,
            types::ErrorCode::HttpRequestLengthRequired => Self::HttpRequestLengthRequired,
            types::ErrorCode::HttpRequestBodySize(size) => Self::HttpRequestBodySize(size),
            types::ErrorCode::HttpRequestMethodInvalid => Self::HttpRequestMethodInvalid,
            types::ErrorCode::HttpRequestUriInvalid => Self::HttpRequestUriInvalid,
            types::ErrorCode::HttpRequestUriTooLong => Self::HttpRequestUriTooLong,
            types::ErrorCode::HttpRequestHeaderSectionSize(err) => {
                Self::HttpRequestHeaderSectionSize(err)
            }
            types::ErrorCode::HttpRequestHeaderSize(err) => {
                Self::HttpRequestHeaderSize(err.map(Into::into))
            }
            types::ErrorCode::HttpRequestTrailerSectionSize(err) => {
                Self::HttpRequestTrailerSectionSize(err)
            }
            types::ErrorCode::HttpRequestTrailerSize(err) => {
                Self::HttpRequestTrailerSize(err.into())
            }
            types::ErrorCode::HttpResponseIncomplete => Self::HttpResponseIncomplete,
            types::ErrorCode::HttpResponseHeaderSectionSize(err) => {
                Self::HttpResponseHeaderSectionSize(err)
            }
            types::ErrorCode::HttpResponseHeaderSize(err) => {
                Self::HttpResponseHeaderSize(err.into())
            }
            types::ErrorCode::HttpResponseBodySize(err) => Self::HttpResponseBodySize(err),
            types::ErrorCode::HttpResponseTrailerSectionSize(err) => {
                Self::HttpResponseTrailerSectionSize(err)
            }
            types::ErrorCode::HttpResponseTrailerSize(err) => {
                Self::HttpResponseTrailerSize(err.into())
            }
            types::ErrorCode::HttpResponseTransferCoding(err) => {
                Self::HttpResponseTransferCoding(err)
            }
            types::ErrorCode::HttpResponseContentCoding(err) => {
                Self::HttpResponseContentCoding(err)
            }
            types::ErrorCode::HttpResponseTimeout => Self::HttpResponseTimeout,
            types::ErrorCode::HttpUpgradeFailed => Self::HttpUpgradeFailed,
            types::ErrorCode::HttpProtocolError => Self::HttpProtocolError,
            types::ErrorCode::LoopDetected => Self::LoopDetected,
            types::ErrorCode::ConfigurationError => Self::ConfigurationError,
            types::ErrorCode::InternalError(err) => Self::InternalError(err),
        }
    }
}

#[cfg(feature = "wasmtime-wasi-http")]
impl From<bindings::wasi::http::types::ErrorCode>
    for wasmtime_wasi_http::bindings::http::types::ErrorCode
{
    fn from(code: bindings::wasi::http::types::ErrorCode) -> Self {
        use bindings::wasi::http::types::ErrorCode;

        match code {
            ErrorCode::DnsTimeout => Self::DnsTimeout,
            ErrorCode::DnsError(err) => Self::DnsError(err.into()),
            ErrorCode::DestinationNotFound => Self::DestinationNotFound,
            ErrorCode::DestinationUnavailable => Self::DestinationUnavailable,
            ErrorCode::DestinationIpProhibited => Self::DestinationIpProhibited,
            ErrorCode::DestinationIpUnroutable => Self::DestinationIpUnroutable,
            ErrorCode::ConnectionRefused => Self::ConnectionRefused,
            ErrorCode::ConnectionTerminated => Self::ConnectionTerminated,
            ErrorCode::ConnectionTimeout => Self::ConnectionTimeout,
            ErrorCode::ConnectionReadTimeout => Self::ConnectionReadTimeout,
            ErrorCode::ConnectionWriteTimeout => Self::ConnectionWriteTimeout,
            ErrorCode::ConnectionLimitReached => Self::ConnectionLimitReached,
            ErrorCode::TlsProtocolError => Self::TlsProtocolError,
            ErrorCode::TlsCertificateError => Self::TlsCertificateError,
            ErrorCode::TlsAlertReceived(err) => Self::TlsAlertReceived(err.into()),
            ErrorCode::HttpRequestDenied => Self::HttpRequestDenied,
            ErrorCode::HttpRequestLengthRequired => Self::HttpRequestLengthRequired,
            ErrorCode::HttpRequestBodySize(size) => Self::HttpRequestBodySize(size),
            ErrorCode::HttpRequestMethodInvalid => Self::HttpRequestMethodInvalid,
            ErrorCode::HttpRequestUriInvalid => Self::HttpRequestUriInvalid,
            ErrorCode::HttpRequestUriTooLong => Self::HttpRequestUriTooLong,
            ErrorCode::HttpRequestHeaderSectionSize(err) => Self::HttpRequestHeaderSectionSize(err),
            ErrorCode::HttpRequestHeaderSize(err) => {
                Self::HttpRequestHeaderSize(err.map(Into::into))
            }
            ErrorCode::HttpRequestTrailerSectionSize(err) => {
                Self::HttpRequestTrailerSectionSize(err)
            }
            ErrorCode::HttpRequestTrailerSize(err) => Self::HttpRequestTrailerSize(err.into()),
            ErrorCode::HttpResponseIncomplete => Self::HttpResponseIncomplete,
            ErrorCode::HttpResponseHeaderSectionSize(err) => {
                Self::HttpResponseHeaderSectionSize(err)
            }
            ErrorCode::HttpResponseHeaderSize(err) => Self::HttpResponseHeaderSize(err.into()),
            ErrorCode::HttpResponseBodySize(err) => Self::HttpResponseBodySize(err),
            ErrorCode::HttpResponseTrailerSectionSize(err) => {
                Self::HttpResponseTrailerSectionSize(err)
            }
            ErrorCode::HttpResponseTrailerSize(err) => Self::HttpResponseTrailerSize(err.into()),
            ErrorCode::HttpResponseTransferCoding(err) => Self::HttpResponseTransferCoding(err),
            ErrorCode::HttpResponseContentCoding(err) => Self::HttpResponseContentCoding(err),
            ErrorCode::HttpResponseTimeout => Self::HttpResponseTimeout,
            ErrorCode::HttpUpgradeFailed => Self::HttpUpgradeFailed,
            ErrorCode::HttpProtocolError => Self::HttpProtocolError,
            ErrorCode::LoopDetected => Self::LoopDetected,
            ErrorCode::ConfigurationError => Self::ConfigurationError,
            ErrorCode::InternalError(err) => Self::InternalError(err),
        }
    }
}

#[cfg(feature = "http-body")]
pub struct HttpBody {
    pub body: core::pin::Pin<Box<dyn futures::Stream<Item = bytes::Bytes> + Send + Sync>>,
    pub trailers: core::pin::Pin<
        Box<
            dyn core::future::Future<Output = Option<bindings::wrpc::http::types::Fields>>
                + Send
                + Sync,
        >,
    >,
}

#[cfg(feature = "http-body")]
impl http_body::Body for HttpBody {
    type Data = bytes::Bytes;
    type Error = anyhow::Error;

    fn poll_frame(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        use anyhow::Context as _;
        use core::task::Poll;
        use futures::{FutureExt as _, StreamExt as _};

        match self.body.poll_next_unpin(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some(buf)) => Poll::Ready(Some(Ok(http_body::Frame::data(buf)))),
            Poll::Ready(None) => match self.trailers.poll_unpin(cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Ready(Some(trailers)) => {
                    let trailers = try_fields_to_header_map(trailers)
                        .context("failed to convert trailer fields to header map")?;
                    Poll::Ready(Some(Ok(http_body::Frame::trailers(trailers))))
                }
            },
        }
    }
}

#[cfg(feature = "http-body")]
pub enum HttpBodyError<E> {
    InvalidFrame,
    TrailerReceiverClosed,
    HeaderConversion(anyhow::Error),
    Body(E),
}

#[cfg(feature = "http-body")]
impl<E: core::fmt::Debug> core::fmt::Debug for HttpBodyError<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidFrame => write!(f, "frame is not valid"),
            Self::TrailerReceiverClosed => write!(f, "trailer receiver is closed"),
            Self::HeaderConversion(err) => write!(f, "failed to convert headers: {err:#}"),
            Self::Body(err) => write!(f, "encountered a body error: {err:?}"),
        }
    }
}

#[cfg(feature = "http-body")]
impl<E: core::fmt::Display> core::fmt::Display for HttpBodyError<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidFrame => write!(f, "frame is not valid"),
            Self::TrailerReceiverClosed => write!(f, "trailer receiver is closed"),
            Self::HeaderConversion(err) => write!(f, "failed to convert headers: {err:#}"),
            Self::Body(err) => write!(f, "encountered a body error: {err}"),
        }
    }
}

#[cfg(feature = "http-body")]
impl<E: core::fmt::Debug + core::fmt::Display> std::error::Error for HttpBodyError<E> {}

#[cfg(feature = "http-body")]
#[tracing::instrument(level = "trace", skip_all)]
pub fn split_http_body<E>(
    body: impl http_body::Body<Data = bytes::Bytes, Error = E>,
) -> (
    impl futures::Stream<Item = Result<bytes::Bytes, HttpBodyError<E>>>,
    impl core::future::Future<Output = Option<bindings::wrpc::http::types::Fields>>,
) {
    use futures::StreamExt as _;

    let (trailers_tx, mut trailers_rx) = tokio::sync::mpsc::channel(1);
    let body = http_body_util::BodyStream::new(body).filter_map(move |frame| {
        let trailers_tx = trailers_tx.clone();
        async move {
            match frame {
                Ok(frame) => match frame.into_data() {
                    Ok(buf) => Some(Ok(buf)),
                    Err(trailers) => match trailers.into_trailers() {
                        Ok(trailers) => match try_header_map_to_fields(trailers) {
                            Ok(trailers) => {
                                if trailers_tx.send(trailers).await.is_err() {
                                    Some(Err(HttpBodyError::TrailerReceiverClosed))
                                } else {
                                    None
                                }
                            }
                            Err(err) => Some(Err(HttpBodyError::HeaderConversion(err))),
                        },
                        Err(_) => Some(Err(HttpBodyError::InvalidFrame)),
                    },
                },
                Err(err) => Some(Err(HttpBodyError::Body(err))),
            }
        }
    });
    let trailers = async move { trailers_rx.recv().await };
    (body, trailers)
}

#[cfg(feature = "http-body")]
#[tracing::instrument(level = "trace", skip_all)]
pub fn split_outgoing_http_body<E>(
    body: impl http_body::Body<Data = bytes::Bytes, Error = E>,
) -> (
    impl futures::Stream<Item = bytes::Bytes>,
    impl core::future::Future<Output = Option<bindings::wrpc::http::types::Fields>>,
    impl futures::Stream<Item = HttpBodyError<E>>,
) {
    use futures::StreamExt as _;

    let (body, trailers) = split_http_body(body);
    let (errors_tx, errors_rx) = tokio::sync::mpsc::channel(1);
    let body = body.filter_map(move |res| {
        let errors_tx = errors_tx.clone();
        async move {
            match res {
                Ok(buf) => Some(buf),
                Err(err) => {
                    if errors_tx.send(err).await.is_err() {
                        tracing::trace!("failed to send body error");
                    }
                    None
                }
            }
        }
    });
    let errors_rx = tokio_stream::wrappers::ReceiverStream::new(errors_rx);
    (body, trailers, errors_rx)
}

#[cfg(feature = "http-body")]
impl TryFrom<bindings::wrpc::http::types::Request> for http::Request<HttpBody> {
    type Error = anyhow::Error;

    fn try_from(
        bindings::wrpc::http::types::Request {
            body,
            trailers,
            method,
            path_with_query,
            scheme,
            authority,
            headers,
        }: bindings::wrpc::http::types::Request,
    ) -> Result<Self, Self::Error> {
        use anyhow::Context as _;

        let uri = http::Uri::builder();
        let uri = if let Some(path_with_query) = path_with_query {
            uri.path_and_query(path_with_query)
        } else {
            uri.path_and_query("/")
        };
        let uri = if let Some(scheme) = scheme {
            let scheme =
                http::uri::Scheme::try_from(&scheme).context("failed to convert scheme")?;
            uri.scheme(scheme)
        } else {
            uri
        };
        let uri = if let Some(authority) = authority {
            uri.authority(authority)
        } else {
            uri
        };
        let uri = uri.build().context("failed to build URI")?;
        let method = http::method::Method::try_from(&method).context("failed to convert method")?;
        let mut req = http::Request::builder().method(method).uri(uri);
        let req_headers = req
            .headers_mut()
            .context("failed to construct header map")?;
        *req_headers = try_fields_to_header_map(headers)
            .context("failed to convert header fields to header map")?;
        req.body(HttpBody { body, trailers })
            .context("failed to construct request")
    }
}

#[cfg(feature = "wasmtime-wasi-http")]
impl TryFrom<bindings::wrpc::http::types::Request>
    for http::Request<
        http_body_util::combinators::BoxBody<
            bytes::Bytes,
            wasmtime_wasi_http::bindings::http::types::ErrorCode,
        >,
    >
{
    type Error = anyhow::Error;

    fn try_from(req: bindings::wrpc::http::types::Request) -> Result<Self, Self::Error> {
        use http_body_util::BodyExt as _;

        let req: http::Request<HttpBody> = req.try_into()?;
        Ok(req.map(|HttpBody { body, trailers }| {
            http_body_util::combinators::BoxBody::new(HttpBody { body, trailers }.map_err(|err| {
                wasmtime_wasi_http::bindings::http::types::ErrorCode::InternalError(Some(format!(
                    "{err:#}"
                )))
            }))
        }))
    }
}

/// Attempt converting [`http::Request`] to [`Request`].
/// Values of `path_with_query`, `scheme` and `authority` will be taken from the
/// request URI
#[cfg(feature = "http-body")]
#[tracing::instrument(level = "trace", skip_all)]
pub fn try_http_to_request<E>(
    request: http::Request<
        impl http_body::Body<Data = bytes::Bytes, Error = E> + Send + Sync + 'static,
    >,
) -> anyhow::Result<(
    bindings::wrpc::http::types::Request,
    impl futures::Stream<Item = HttpBodyError<E>>,
)>
where
    E: Send + Sync + 'static,
{
    let (
        http::request::Parts {
            ref method,
            uri,
            headers,
            ..
        },
        body,
    ) = request.into_parts();
    let headers = try_header_map_to_fields(headers)?;
    let (body, trailers, errors) = split_outgoing_http_body(body);
    Ok((
        bindings::wrpc::http::types::Request {
            body: Box::pin(body),
            trailers: Box::pin(trailers),
            method: method.into(),
            path_with_query: uri.path_and_query().map(ToString::to_string),
            scheme: uri.scheme().map(Into::into),
            authority: uri.authority().map(ToString::to_string),
            headers,
        },
        errors,
    ))
}

/// Attempt converting [`http::Request<wasmtime_wasi_http::body::HyperOutgoingBody>`] to [`bindings::wrpc::http::types::Request`].
/// Values of `path_with_query`, `scheme` and `authority` will be taken from the
/// request URI.
#[cfg(feature = "wasmtime-wasi-http")]
#[tracing::instrument(level = "trace", skip_all)]
pub fn try_wasmtime_to_outgoing_request(
    req: http::Request<wasmtime_wasi_http::body::HyperOutgoingBody>,
    wasmtime_wasi_http::types::OutgoingRequestConfig {
        use_tls: _,
        connect_timeout,
        first_byte_timeout,
        between_bytes_timeout,
    }: wasmtime_wasi_http::types::OutgoingRequestConfig,
) -> anyhow::Result<(
    bindings::wrpc::http::types::Request,
    bindings::wrpc::http::types::RequestOptions,
    impl futures::Stream<Item = HttpBodyError<wasmtime_wasi_http::bindings::http::types::ErrorCode>>
        + Send,
)> {
    use anyhow::Context as _;

    let (req, errors) = try_http_to_request(req)?;
    let connect_timeout = connect_timeout
        .as_nanos()
        .try_into()
        .context("`connect_timeout` nanoseconds do not fit in u64")?;
    let first_byte_timeout = first_byte_timeout
        .as_nanos()
        .try_into()
        .context("`first_byte_timeout` nanoseconds do not fit in u64")?;
    let between_bytes_timeout = between_bytes_timeout
        .as_nanos()
        .try_into()
        .context("`between_bytes_timeout` nanoseconds do not fit in u64")?;
    Ok((
        req,
        bindings::wrpc::http::types::RequestOptions {
            connect_timeout: Some(connect_timeout),
            first_byte_timeout: Some(first_byte_timeout),
            between_bytes_timeout: Some(between_bytes_timeout),
        },
        errors,
    ))
}

#[cfg(feature = "http-body")]
impl TryFrom<bindings::wrpc::http::types::Response> for http::Response<HttpBody> {
    type Error = anyhow::Error;

    fn try_from(
        bindings::wrpc::http::types::Response {
            body,
            trailers,
            status,
            headers,
        }: bindings::wrpc::http::types::Response,
    ) -> Result<Self, Self::Error> {
        use anyhow::Context as _;

        let mut resp = http::Response::builder().status(status);
        let resp_headers = resp
            .headers_mut()
            .context("failed to construct header map")?;
        *resp_headers = try_fields_to_header_map(headers)
            .context("failed to convert header fields to header map")?;
        resp.body(HttpBody { body, trailers })
            .context("failed to construct response")
    }
}

#[cfg(feature = "wasmtime-wasi-http")]
impl TryFrom<bindings::wrpc::http::types::Response>
    for http::Response<
        http_body_util::combinators::BoxBody<
            bytes::Bytes,
            wasmtime_wasi_http::bindings::http::types::ErrorCode,
        >,
    >
{
    type Error = anyhow::Error;

    fn try_from(
        bindings::wrpc::http::types::Response {
            body,
            trailers,
            status,
            headers,
        }: bindings::wrpc::http::types::Response,
    ) -> Result<Self, Self::Error> {
        use anyhow::Context as _;
        use http_body_util::BodyExt as _;

        let mut resp = http::Response::builder().status(status);
        let resp_headers = resp
            .headers_mut()
            .context("failed to construct header map")?;
        *resp_headers = try_fields_to_header_map(headers)
            .context("failed to convert header fields to header map")?;
        let trailers = tokio::spawn(trailers);
        resp.body(http_body_util::combinators::BoxBody::new(
            HttpBody {
                body,
                trailers: Box::pin(async { trailers.await.unwrap() }),
            }
            .map_err(|err| {
                wasmtime_wasi_http::bindings::http::types::ErrorCode::InternalError(Some(format!(
                    "{err:#}"
                )))
            }),
        ))
        .context("failed to construct response")
    }
}

#[cfg(feature = "http-body")]
#[tracing::instrument(level = "trace", skip_all)]
pub fn try_http_to_response<E>(
    response: http::Response<
        impl http_body::Body<Data = bytes::Bytes, Error = E> + Send + Sync + 'static,
    >,
) -> anyhow::Result<(
    bindings::wrpc::http::types::Response,
    impl futures::Stream<Item = HttpBodyError<E>>,
)>
where
    E: Send + Sync + 'static,
{
    let (
        http::response::Parts {
            status, headers, ..
        },
        body,
    ) = response.into_parts();
    let headers = try_header_map_to_fields(headers)?;
    let (body, trailers, errors) = split_outgoing_http_body(body);
    Ok((
        bindings::wrpc::http::types::Response {
            body: Box::pin(body),
            trailers: Box::pin(trailers),
            status: status.into(),
            headers,
        },
        errors,
    ))
}

pub trait InvokeIncomingHandler: wrpc_transport::Invoke {
    #[cfg(feature = "http-body")]
    #[tracing::instrument(level = "trace", skip_all)]
    fn invoke_handle_http<E>(
        &self,
        cx: Self::Context,
        request: http::Request<
            impl http_body::Body<Data = bytes::Bytes, Error = E> + Send + Sync + 'static,
        >,
    ) -> impl core::future::Future<
        Output = anyhow::Result<(
            Result<http::Response<HttpBody>, bindings::wrpc::http::types::ErrorCode>,
            impl futures::Stream<Item = HttpBodyError<E>>,
            Option<impl core::future::Future<Output = anyhow::Result<()>>>,
        )>,
    >
    where
        Self: Sized,
        E: Send + Sync + 'static,
    {
        use anyhow::Context as _;

        async {
            let (request, errors) = try_http_to_request(request)
                .context("failed to convert `http` request to `wrpc:http/types.request`")?;
            let (response, io) = bindings::wrpc::http::incoming_handler::handle(self, cx, request)
                .await
                .context("failed to invoke `wrpc:http/incoming-handler.handle`")?;
            match response {
                Ok(response) => {
                    let response = response.try_into().context(
                        "failed to convert `wrpc:http/types.response` to `http` response",
                    )?;
                    Ok((Ok(response), errors, io))
                }
                Err(code) => Ok((Err(code), errors, io)),
            }
        }
    }
}

impl<T: wrpc_transport::Invoke> InvokeIncomingHandler for T {}

pub trait InvokeOutgoingHandler: wrpc_transport::Invoke {
    #[cfg(feature = "wasmtime-wasi-http")]
    #[tracing::instrument(level = "trace", skip_all)]
    fn invoke_handle_wasmtime(
        &self,
        cx: Self::Context,
        request: http::Request<wasmtime_wasi_http::body::HyperOutgoingBody>,
        options: wasmtime_wasi_http::types::OutgoingRequestConfig,
    ) -> impl core::future::Future<
        Output = anyhow::Result<(
            Result<
                http::Response<wasmtime_wasi_http::body::HyperIncomingBody>,
                wasmtime_wasi_http::bindings::http::types::ErrorCode,
            >,
            impl futures::Stream<
                Item = HttpBodyError<wasmtime_wasi_http::bindings::http::types::ErrorCode>,
            >,
            Option<impl core::future::Future<Output = anyhow::Result<()>>>,
        )>,
    >
    where
        Self: Sized,
    {
        use anyhow::Context as _;

        async {
            let (req, opts, errors) = try_wasmtime_to_outgoing_request(request, options)?;
            let (resp, io) =
                bindings::wrpc::http::outgoing_handler::handle(self, cx, req, Some(opts))
                    .await
                    .context("failed to invoke `wrpc:http/outgoing-handler.handle`")?;
            match resp {
                Ok(resp) => {
                    let resp: http::Response<wasmtime_wasi_http::body::HyperIncomingBody> =
                        resp.try_into().context(
                            "failed to convert `wrpc:http` response to Wasmtime `wasi:http`",
                        )?;
                    Ok((Ok(resp), errors, io))
                }
                Err(code) => Ok((Err(code.into()), errors, io)),
            }
        }
    }
}

impl<T: wrpc_transport::Invoke> InvokeOutgoingHandler for T {}

#[cfg(feature = "http-body")]
#[derive(Copy, Clone, Debug)]
pub struct ServeHttp<T>(pub T);

#[cfg(feature = "http-body")]
pub trait ServeIncomingHandlerHttp<Ctx> {
    fn handle(
        &self,
        cx: Ctx,
        request: http::Request<HttpBody>,
    ) -> impl core::future::Future<
        Output = anyhow::Result<
            Result<
                http::Response<
                    impl http_body::Body<Data = bytes::Bytes, Error = core::convert::Infallible>
                        + Send
                        + Sync
                        + 'static,
                >,
                bindings::wasi::http::types::ErrorCode,
            >,
        >,
    >;
}

#[cfg(feature = "http-body")]
impl<Ctx, T> bindings::exports::wrpc::http::incoming_handler::Handler<Ctx> for ServeHttp<T>
where
    T: ServeIncomingHandlerHttp<Ctx>,
{
    async fn handle(
        &self,
        cx: Ctx,
        request: bindings::wrpc::http::types::Request,
    ) -> anyhow::Result<
        Result<bindings::wrpc::http::types::Response, bindings::wasi::http::types::ErrorCode>,
    > {
        use anyhow::Context as _;
        use futures::StreamExt as _;

        let request = request
            .try_into()
            .context("failed to convert incoming `wrpc:http/types.request` to `http` request")?;
        match ServeIncomingHandlerHttp::handle(&self.0, cx, request).await? {
            Ok(response) => {
                let (response, errs) = try_http_to_response(response).context(
                    "failed to convert outgoing `http` response to `wrpc:http/types.response`",
                )?;
                tokio::spawn(errs.for_each_concurrent(None, |err| async move {
                    tracing::error!(?err, "response body processing error encountered");
                }));
                Ok(Ok(response))
            }
            Err(code) => Ok(Err(code)),
        }
    }
}

#[cfg(feature = "wasmtime-wasi-http")]
#[derive(Copy, Clone, Debug)]
pub struct ServeWasmtime<T>(pub T);

#[cfg(feature = "wasmtime-wasi-http")]
pub trait ServeIncomingHandlerWasmtime<Ctx> {
    fn handle(
        &self,
        cx: Ctx,
        request: http::Request<wasmtime_wasi_http::body::HyperIncomingBody>,
    ) -> impl core::future::Future<
        Output = anyhow::Result<
            Result<
                http::Response<wasmtime_wasi_http::body::HyperOutgoingBody>,
                wasmtime_wasi_http::bindings::http::types::ErrorCode,
            >,
        >,
    >;
}

#[cfg(feature = "wasmtime-wasi-http")]
impl<Ctx, T> bindings::exports::wrpc::http::incoming_handler::Handler<Ctx> for ServeWasmtime<T>
where
    T: ServeIncomingHandlerWasmtime<Ctx>,
{
    async fn handle(
        &self,
        cx: Ctx,
        request: bindings::wrpc::http::types::Request,
    ) -> anyhow::Result<
        Result<bindings::wrpc::http::types::Response, bindings::wasi::http::types::ErrorCode>,
    > {
        use anyhow::Context as _;
        use futures::StreamExt as _;

        let request = request.try_into().context(
            "failed to convert incoming `wrpc:http/types.request` to `wasmtime-wasi-http` request",
        )?;
        match ServeIncomingHandlerWasmtime::handle(&self.0, cx, request).await? {
            Ok(response) => {
                let (response, errs) = try_http_to_response(response).context(
                    "failed to convert outgoing `http` response to `wrpc:http/types.response`",
                )?;
                tokio::spawn(errs.for_each_concurrent(None, |err| async move {
                    tracing::error!(?err, "response body processing error encountered");
                }));
                Ok(Ok(response))
            }
            Err(code) => Ok(Err(code.into())),
        }
    }
}

#[cfg(feature = "http-body")]
pub trait ServeOutgoingHandlerHttp<Ctx> {
    fn handle(
        &self,
        cx: Ctx,
        request: http::Request<HttpBody>,
        options: Option<bindings::wrpc::http::types::RequestOptions>,
    ) -> impl core::future::Future<
        Output = anyhow::Result<
            Result<
                http::Response<
                    impl http_body::Body<Data = bytes::Bytes, Error = core::convert::Infallible>
                        + Send
                        + Sync
                        + 'static,
                >,
                bindings::wasi::http::types::ErrorCode,
            >,
        >,
    >;
}

#[cfg(feature = "http-body")]
impl<Ctx, T> bindings::exports::wrpc::http::outgoing_handler::Handler<Ctx> for ServeHttp<T>
where
    T: ServeOutgoingHandlerHttp<Ctx>,
{
    async fn handle(
        &self,
        cx: Ctx,
        request: bindings::wrpc::http::types::Request,
        options: Option<bindings::wrpc::http::types::RequestOptions>,
    ) -> anyhow::Result<
        Result<bindings::wrpc::http::types::Response, bindings::wasi::http::types::ErrorCode>,
    > {
        use anyhow::Context as _;
        use futures::StreamExt as _;

        let request = request
            .try_into()
            .context("failed to convert incoming `wrpc:http/types.request` to `http` request")?;
        match ServeOutgoingHandlerHttp::handle(&self.0, cx, request, options).await? {
            Ok(response) => {
                let (response, errs) = try_http_to_response(response).context(
                    "failed to convert outgoing `http` response to `wrpc:http/types.response`",
                )?;
                tokio::spawn(errs.for_each_concurrent(None, |err| async move {
                    tracing::error!(?err, "response body processing error encountered");
                }));
                Ok(Ok(response))
            }
            Err(code) => Ok(Err(code)),
        }
    }
}
