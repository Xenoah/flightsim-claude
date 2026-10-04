use std::{
    io::Read,
    net::ToSocketAddrs,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use ureq::{
    Agent,
    config::Config,
    http::{HeaderMap, Uri},
    unversioned::{
        resolver::{ResolvedSocketAddrs, Resolver},
        transport::{
            Buffers, ConnectionDetails, Connector, LazyBuffers, NextTimeout, RustlsConnector,
            TcpConnector, Transport,
        },
    },
};
use url::Url;

use super::{
    DownloadError, Result,
    source::{allowed_host, public_address},
};

pub(super) const IO_TIMEOUT: Duration = Duration::from_secs(5);
pub(super) const NETWORK_TIMEOUT: Duration = Duration::from_secs(180);
const MAX_HEADERS: usize = 16 * 1024;

#[derive(Debug, Default)]
struct PublicResolver;
impl Resolver for PublicResolver {
    fn resolve(
        &self,
        uri: &Uri,
        config: &Config,
        timeout: NextTimeout,
    ) -> std::result::Result<ResolvedSocketAddrs, ureq::Error> {
        if config.proxy().is_some()
            || uri.scheme_str() != Some("https")
            || !uri.host().is_some_and(allowed_host)
            || uri.port_u16().is_some_and(|p| p != 443)
        {
            return Err(ureq::Error::ConnectionFailed);
        }
        // A system DNS call cannot reliably be interrupted. Keep its single
        // process-wide slot until the actual lookup exits, even after timeout,
        // so repeated retries cannot accumulate stuck DNS threads.
        static DNS_BUSY: OnceLock<Arc<AtomicBool>> = OnceLock::new();
        let gate = DNS_BUSY.get_or_init(|| Arc::new(AtomicBool::new(false)));
        let host = uri.host().expect("validated host").to_owned();
        lookup_bounded(gate, *timeout.after.min(IO_TIMEOUT.into()), move || {
            let mut addresses = PublicResolver.empty();
            for addr in (host.as_str(), 443).to_socket_addrs()? {
                // Reject mixed public/private answers, including addresses beyond
                // the 16 we retain. TcpConnector uses these exact addresses.
                if !public_address(addr.ip()) {
                    return Err(ureq::Error::ConnectionFailed);
                }
                if addresses.len() < 16 {
                    addresses.push(addr);
                }
            }
            if addresses.is_empty() {
                return Err(ureq::Error::HostNotFound);
            }
            Ok(addresses)
        })
    }
}

#[derive(Debug)]
struct DnsSlot(Arc<AtomicBool>);
impl Drop for DnsSlot {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
fn lookup_bounded(
    gate: &Arc<AtomicBool>,
    timeout: Duration,
    lookup: impl FnOnce() -> std::result::Result<ResolvedSocketAddrs, ureq::Error> + Send + 'static,
) -> std::result::Result<ResolvedSocketAddrs, ureq::Error> {
    if gate
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(ureq::Error::ConnectionFailed);
    }
    let slot = DnsSlot(gate.clone());
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("region-dns".into())
        .spawn(move || {
            let _slot = slot;
            let _ = sender.send(lookup());
        })?;
    receiver
        .recv_timeout(timeout)
        .map_err(|_| ureq::Error::Timeout(ureq::Timeout::Resolve))?
}

/// Cap each transport wait in addition to the end-to-end request deadline.
/// A stalled stream terminates, allowing a callback/worker to observe cancellation.
#[derive(Debug)]
struct IdleLimit {
    deadline: Instant,
}
impl<T: Transport> Connector<T> for IdleLimit {
    type Out = LimitedTransport<T>;
    fn connect(
        &self,
        _: &ConnectionDetails,
        chained: Option<T>,
    ) -> std::result::Result<Option<Self::Out>, ureq::Error> {
        Ok(chained.map(|inner| LimitedTransport {
            inner,
            deadline: self.deadline,
        }))
    }
}
#[derive(Debug)]
struct LimitedTransport<T> {
    inner: T,
    deadline: Instant,
}
impl<T: Transport> LimitedTransport<T> {
    fn bounded(&self, mut timeout: NextTimeout) -> std::result::Result<NextTimeout, ureq::Error> {
        let remaining = self
            .deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or(ureq::Error::Timeout(ureq::Timeout::Global))?;
        timeout.after = timeout.after.min(IO_TIMEOUT.into()).min(remaining.into());
        Ok(timeout)
    }
}
impl<T: Transport> Transport for LimitedTransport<T> {
    fn buffers(&mut self) -> &mut dyn Buffers {
        self.inner.buffers()
    }
    fn transmit_output(
        &mut self,
        n: usize,
        t: NextTimeout,
    ) -> std::result::Result<(), ureq::Error> {
        let bounded = self.bounded(t)?;
        self.inner.transmit_output(n, bounded)
    }
    fn await_input(&mut self, t: NextTimeout) -> std::result::Result<bool, ureq::Error> {
        let bounded = self.bounded(t)?;
        self.inner.await_input(bounded)
    }
    fn is_open(&mut self) -> bool {
        self.inner.is_open()
    }
    fn is_tls(&self) -> bool {
        self.inner.is_tls()
    }
}

// ureq strips Content-Encoding/Length before returning a response when gzip is
// feature-unified by another crate. Reject encoded responses at the plaintext
// transport boundary, before ureq's parser can construct any content decoder.
#[derive(Debug)]
struct IdentityHeaders;
impl<T: Transport> Connector<T> for IdentityHeaders {
    type Out = HeaderTransport<T>;
    fn connect(
        &self,
        details: &ConnectionDetails,
        chained: Option<T>,
    ) -> std::result::Result<Option<Self::Out>, ureq::Error> {
        Ok(chained.map(|inner| HeaderTransport {
            inner,
            buffers: LazyBuffers::new(MAX_HEADERS + 1, details.config.output_buffer_size()),
            checked: false,
            header_deadline: Instant::now() + IO_TIMEOUT,
        }))
    }
}
#[derive(Debug)]
struct HeaderTransport<T> {
    inner: T,
    buffers: LazyBuffers,
    checked: bool,
    header_deadline: Instant,
}
fn header_error() -> ureq::Error {
    ureq::Error::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "unsupported download HTTP headers",
    ))
}
fn check_plain_headers(bytes: &[u8]) -> std::result::Result<(), ureq::Error> {
    let mut headers = [httparse::EMPTY_HEADER; 128];
    let mut response = httparse::Response::new(&mut headers);
    if !response
        .parse(bytes)
        .map_err(|_| header_error())?
        .is_complete()
        || response.code.is_none_or(|code| code < 200)
    {
        return Err(header_error());
    }
    let mut map = HeaderMap::new();
    for header in response.headers.iter() {
        map.append(
            ureq::http::HeaderName::from_bytes(header.name.as_bytes())
                .map_err(|_| header_error())?,
            ureq::http::HeaderValue::from_bytes(header.value).map_err(|_| header_error())?,
        );
    }
    // All statuses reject encoded bodies. Text bodies are rejected only for 200:
    // GitHub redirect responses may carry a small HTML body, which is never read.
    if map
        .get_all("content-encoding")
        .iter()
        .any(|v| v.as_bytes() != b"identity")
    {
        return Err(header_error());
    }
    if response.code == Some(200) {
        validate_encoding(&map).map_err(|_| header_error())?;
    }
    let length = content_length(&map).map_err(|_| header_error())?;
    if length.is_some_and(|n| n > crate::MAX_ARCHIVE_BYTES) {
        return Err(header_error());
    }
    let mut encodings = map.get_all("transfer-encoding").iter();
    if let Some(value) = encodings.next() {
        if !value.as_bytes().eq_ignore_ascii_case(b"chunked") || encodings.next().is_some() {
            return Err(header_error());
        }
    }
    Ok(())
}
impl<T: Transport> Transport for HeaderTransport<T> {
    fn buffers(&mut self) -> &mut dyn Buffers {
        &mut self.buffers
    }
    fn transmit_output(
        &mut self,
        amount: usize,
        timeout: NextTimeout,
    ) -> std::result::Result<(), ureq::Error> {
        let mut sent = 0;
        while sent < amount {
            let target = self.inner.buffers().output();
            let n = target.len().min(amount - sent);
            target[..n].copy_from_slice(&self.buffers.output()[sent..sent + n]);
            self.inner.transmit_output(n, timeout)?;
            sent += n;
        }
        Ok(())
    }
    fn await_input(&mut self, mut timeout: NextTimeout) -> std::result::Result<bool, ureq::Error> {
        loop {
            if !self.checked {
                let remaining = self
                    .header_deadline
                    .checked_duration_since(Instant::now())
                    .filter(|d| !d.is_zero())
                    .ok_or(ureq::Error::Timeout(ureq::Timeout::RecvResponse))?;
                timeout.after = timeout.after.min(remaining.into());
            }
            self.inner.maybe_await_input(timeout)?;
            let input = self.inner.buffers().input();
            let target = self.buffers.input_append_buf();
            let n = target.len().min(input.len());
            if n == 0 {
                if !input.is_empty() {
                    return Err(header_error());
                }
                return if self.checked {
                    Ok(false)
                } else {
                    Err(header_error())
                };
            }
            target[..n].copy_from_slice(&input[..n]);
            self.inner.buffers().input_consume(n);
            self.buffers.input_appended(n);
            if self.checked {
                return Ok(true);
            }
            let bytes = self.buffers.input();
            if let Some(end) = bytes
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .map(|p| p + 4)
            {
                if end > MAX_HEADERS {
                    return Err(ureq::Error::LargeResponseHeader(MAX_HEADERS, end));
                }
                check_plain_headers(&bytes[..end])?;
                self.checked = true;
                return Ok(true);
            }
            if bytes.len() > MAX_HEADERS {
                return Err(ureq::Error::LargeResponseHeader(MAX_HEADERS, bytes.len()));
            }
        }
    }
    fn is_open(&mut self) -> bool {
        self.inner.is_open()
    }
    fn is_tls(&self) -> bool {
        self.inner.is_tls()
    }
}

pub(super) struct Response {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: Box<dyn Read>,
}

// A private seam for deterministic hostile responses. Callers cannot replace the
// resolver/client or bypass the production URL and network policy.
pub(super) trait Http {
    fn get(&mut self, url: &Url, remaining: Duration) -> Result<Response>;
}
#[derive(Debug)]
pub(super) struct GitHubHttp;
fn request_config(remaining: Duration) -> Config {
    Agent::config_builder()
        .https_only(true)
        .proxy(None)
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(remaining))
        .timeout_resolve(Some(IO_TIMEOUT))
        .timeout_connect(Some(IO_TIMEOUT))
        .timeout_send_request(Some(IO_TIMEOUT))
        .timeout_recv_response(Some(IO_TIMEOUT))
        .max_response_header_size(MAX_HEADERS)
        .max_idle_connections(0)
        .user_agent("flightsim-content/1")
        .build()
}
impl Http for GitHubHttp {
    fn get(&mut self, url: &Url, remaining: Duration) -> Result<Response> {
        let config = request_config(remaining);
        // Fresh per hop: no cookies, auth, client-supplied headers, proxy, redirect
        // history or pooled sockets survive into a subsequent request.
        let connector = ()
            .chain(TcpConnector::default())
            .chain(IdleLimit {
                deadline: Instant::now() + remaining,
            })
            .chain(RustlsConnector::default())
            .chain(IdentityHeaders);
        let agent = Agent::with_parts(config, connector, PublicResolver);
        let response = agent
            .get(url.as_str())
            .header("Accept", "application/zip, application/octet-stream")
            .header("Accept-Encoding", "identity")
            .header("Connection", "close")
            .call()
            .map_err(|e| network_error(&e))?;
        let (parts, body) = response.into_parts();
        // Inspect headers before constructing any automatic content decoder.
        // Feature unification elsewhere may enable gzip/charset support in ureq.
        if parts.status.as_u16() == 200 {
            validate_encoding(&parts.headers)?;
        }
        Ok(Response {
            status: parts.status.as_u16(),
            headers: parts.headers,
            body: Box::new(body.into_reader()),
        })
    }
}

pub(super) fn validate_encoding(headers: &HeaderMap) -> Result<()> {
    if headers
        .get_all("content-encoding")
        .iter()
        .any(|v| v.as_bytes() != b"identity")
    {
        return Err(DownloadError::Network("encoded HTTP body is unsupported"));
    }
    if headers.get_all("content-type").iter().any(|v| {
        v.to_str().map_or(true, |s| {
            s.split(';')
                .next()
                .is_some_and(|t| t.trim().to_ascii_lowercase().starts_with("text/"))
        })
    }) {
        return Err(DownloadError::Network(
            "text HTTP body is not a prepared ZIP",
        ));
    }
    Ok(())
}

pub(super) fn content_length(headers: &HeaderMap) -> Result<Option<u64>> {
    let mut values = headers.get_all("content-length").iter();
    let value = values.next();
    if values.next().is_some() || (value.is_some() && headers.contains_key("transfer-encoding")) {
        return Err(DownloadError::Network("ambiguous HTTP body length"));
    }
    value
        .map(|v| {
            let s = v
                .to_str()
                .map_err(|_| DownloadError::Network("invalid Content-Length"))?;
            if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
                return Err(DownloadError::Network("invalid Content-Length"));
            }
            s.parse()
                .map_err(|_| DownloadError::Network("invalid Content-Length"))
        })
        .transpose()
}
fn network_error(error: &ureq::Error) -> DownloadError {
    // Never expose signed redirect URLs or arbitrary server strings in diagnostics.
    DownloadError::Network(match error {
        ureq::Error::Timeout(_) => "request timed out; retry explicitly",
        ureq::Error::HostNotFound => "public GitHub host could not be resolved",
        ureq::Error::ConnectionFailed => "connection or public-address policy failed",
        ureq::Error::Rustls(_) | ureq::Error::Tls(_) => "TLS verification/connection failed",
        ureq::Error::LargeResponseHeader(_, _) => "HTTP response headers exceed limit",
        _ => "HTTP transport failed; retry explicitly",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    use ureq::unversioned::transport::time;

    #[test]
    fn a_timed_out_dns_job_keeps_its_slot_until_actual_lookup_finishes() {
        let gate = Arc::new(AtomicBool::new(false));
        let (release, wait) = mpsc::sync_channel(1);
        let error = lookup_bounded(&gate, Duration::from_millis(10), move || {
            wait.recv().unwrap();
            Err(ureq::Error::HostNotFound)
        })
        .unwrap_err();
        assert!(matches!(error, ureq::Error::Timeout(_)));
        assert!(gate.load(Ordering::Acquire));
        assert!(matches!(
            lookup_bounded(&gate, Duration::from_millis(10), || panic!(
                "must not start another DNS thread"
            )),
            Err(ureq::Error::ConnectionFailed)
        ));
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while gate.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(matches!(
            lookup_bounded(&gate, Duration::from_secs(2), || Err(
                ureq::Error::HostNotFound
            )),
            Err(ureq::Error::HostNotFound)
        ));
    }

    #[test]
    fn resolver_rejects_proxy_hosts_and_non_https_before_dns() {
        let timeout = NextTimeout {
            after: time::Duration::from_millis(1),
            reason: ureq::Timeout::Resolve,
        };
        let config = Agent::config_builder().proxy(None).build();
        for url in [
            "http://github.com/a",
            "https://127.0.0.1/a",
            "https://evil.test/a",
            "https://github.com:444/a",
        ] {
            assert!(matches!(
                PublicResolver.resolve(&url.parse().unwrap(), &config, timeout),
                Err(ureq::Error::ConnectionFailed)
            ));
        }
        let config = Agent::config_builder()
            .proxy(Some(ureq::Proxy::new("http://127.0.0.1:8888").unwrap()))
            .build();
        assert!(matches!(
            PublicResolver.resolve(&"https://github.com/a".parse().unwrap(), &config, timeout),
            Err(ureq::Error::ConnectionFailed)
        ));
    }

    fn loopback_response(
        raw: Vec<u8>,
    ) -> std::result::Result<ureq::http::Response<ureq::Body>, ureq::Error> {
        // Test-only HTTP server/route. The public downloader cannot inject this
        // resolver, URL, client or plaintext scheme.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = [0; 4096];
            let _ = stream.read(&mut request).unwrap();
            let _ = stream.write_all(&raw);
        });
        let config = Agent::config_builder()
            .proxy(None)
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(2)))
            .max_response_header_size(MAX_HEADERS)
            .build();
        let connector = ().chain(TcpConnector::default()).chain(IdleLimit {
            deadline: Instant::now() + Duration::from_secs(2),
        });
        let agent = Agent::with_parts(
            config,
            connector.chain(IdentityHeaders),
            ureq::unversioned::resolver::DefaultResolver::default(),
        );
        agent.get(format!("http://{addr}/fixture")).call()
    }

    #[test]
    fn real_http_parser_bounds_chunked_bytes_and_rejects_truncated_content_length() {
        let mut response = loopback_response(
            b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nabc".to_vec(),
        )
        .unwrap();
        let mut bytes = Vec::new();
        assert!(
            response
                .body_mut()
                .as_reader()
                .read_to_end(&mut bytes)
                .is_err()
        );
        let response = loopback_response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\nabc\r\n3\r\ndef\r\n0\r\n\r\n".to_vec()).unwrap();
        let (parts, body) = response.into_parts();
        struct One(Option<Response>);
        impl Http for One {
            fn get(&mut self, _: &Url, _: Duration) -> Result<Response> {
                Ok(self.0.take().unwrap())
            }
        }
        let mut http = One(Some(Response {
            status: 200,
            headers: parts.headers,
            body: Box::new(body.into_reader()),
        }));
        let root = tempfile::tempdir().unwrap();
        let source = super::super::DownloadSource::github(
            "https://github.com/example/data/releases/download/v1/data.zip",
            &crate::sha256(b"abcdef"),
        )
        .unwrap();
        assert!(matches!(
            super::super::receive(
                &source,
                &root.path().join("out"),
                4,
                &mut |_| true,
                &mut http
            ),
            Err(DownloadError::Limit(_))
        ));
        assert!(std::fs::metadata(root.path().join("out")).unwrap().len() <= 4);
    }

    #[test]
    fn socket_wait_is_capped_by_idle_and_absolute_deadlines() {
        use ureq::unversioned::transport::LazyBuffers;
        #[derive(Debug)]
        struct Capture {
            buffers: LazyBuffers,
            seen: Option<Duration>,
        }
        impl Transport for Capture {
            fn buffers(&mut self) -> &mut dyn Buffers {
                &mut self.buffers
            }
            fn transmit_output(
                &mut self,
                _: usize,
                timeout: NextTimeout,
            ) -> std::result::Result<(), ureq::Error> {
                self.seen = Some(*timeout.after);
                Ok(())
            }
            fn await_input(
                &mut self,
                timeout: NextTimeout,
            ) -> std::result::Result<bool, ureq::Error> {
                self.seen = Some(*timeout.after);
                Ok(true)
            }
            fn is_open(&mut self) -> bool {
                true
            }
        }
        let timeout = NextTimeout {
            after: time::Duration::from_secs(180),
            reason: ureq::Timeout::Global,
        };
        let mut transport = LimitedTransport {
            inner: Capture {
                buffers: LazyBuffers::new(1024, 1024),
                seen: None,
            },
            deadline: Instant::now() + Duration::from_secs(60),
        };
        transport.await_input(timeout).unwrap();
        assert_eq!(transport.inner.seen, Some(IO_TIMEOUT));
        transport.deadline = Instant::now() + Duration::from_millis(50);
        transport.transmit_output(0, timeout).unwrap();
        assert!(transport.inner.seen.unwrap() <= Duration::from_millis(50));
        transport.deadline = Instant::now() - Duration::from_secs(1);
        assert!(matches!(
            transport.await_input(timeout),
            Err(ureq::Error::Timeout(ureq::Timeout::Global))
        ));
    }

    #[test]
    fn production_config_keeps_security_and_header_bounds() {
        let config = request_config(Duration::from_secs(10));
        assert!(config.https_only());
        assert!(config.proxy().is_none());
        assert_eq!(config.max_redirects(), 0);
        assert_eq!(config.max_response_header_size(), MAX_HEADERS);
        assert_eq!(config.timeouts().global, Some(Duration::from_secs(10)));
        let raw = format!(
            "HTTP/1.1 200 OK\r\nX-Oversized: {}\r\nContent-Length: 0\r\n\r\n",
            "x".repeat(MAX_HEADERS)
        );
        assert!(matches!(
            loopback_response(raw.into_bytes()),
            Err(ureq::Error::LargeResponseHeader(_, _))
        ));
    }

    #[test]
    fn wire_encoding_is_rejected_before_ureq_can_strip_headers_or_decode() {
        use flate2::{Compression, write::GzEncoder};
        let zip = include_bytes!("../../tests/data/download-fixture.zip");
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(zip).unwrap();
        let gzip = encoder.finish().unwrap();
        for headers in [
            format!(
                "Content-Encoding: gzip\r\nContent-Length: {}\r\n",
                gzip.len()
            ),
            "Content-Encoding: gzip\r\nContent-Length: 536870913\r\n".into(),
            "Content-Encoding: gzip\r\nTransfer-Encoding: chunked\r\n".into(),
            "Content-Encoding: br\r\n".into(),
            "Content-Encoding: identity\r\nContent-Encoding: gzip\r\n".into(),
            "Content-Length: 1\r\nTransfer-Encoding: chunked\r\n".into(),
        ] {
            let mut wire =
                format!("HTTP/1.1 200 OK\r\n{headers}Connection: close\r\n\r\n").into_bytes();
            wire.extend_from_slice(&gzip);
            assert!(
                matches!(loopback_response(wire), Err(ureq::Error::Io(_))),
                "{headers}"
            );
        }
        // Explicit identity keeps exact archive bytes and the original length.
        let mut wire = format!("HTTP/1.1 200 OK\r\nContent-Encoding: identity\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", zip.len()).into_bytes();
        wire.extend_from_slice(zip);
        let mut response = loopback_response(wire).unwrap();
        assert_eq!(
            content_length(response.headers()).unwrap(),
            Some(zip.len() as u64)
        );
        let mut actual = Vec::new();
        response
            .body_mut()
            .as_reader()
            .read_to_end(&mut actual)
            .unwrap();
        assert_eq!(actual, zip);
        assert!(loopback_response(b"HTTP/1.1 103 Early Hints\r\n\r\nHTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\n\r\n".to_vec()).is_err());
    }

    #[test]
    fn split_plaintext_headers_preserve_bytes_and_enforce_exact_header_cap() {
        use std::collections::VecDeque;
        #[derive(Debug)]
        struct Fragments {
            buffers: LazyBuffers,
            chunks: VecDeque<Vec<u8>>,
        }
        impl Transport for Fragments {
            fn buffers(&mut self) -> &mut dyn Buffers {
                &mut self.buffers
            }
            fn transmit_output(
                &mut self,
                _: usize,
                _: NextTimeout,
            ) -> std::result::Result<(), ureq::Error> {
                unreachable!("response-only fixture")
            }
            fn await_input(&mut self, _: NextTimeout) -> std::result::Result<bool, ureq::Error> {
                let Some(chunk) = self.chunks.pop_front() else {
                    return Ok(false);
                };
                self.buffers.input_append_buf()[..chunk.len()].copy_from_slice(&chunk);
                self.buffers.input_appended(chunk.len());
                Ok(true)
            }
            fn is_open(&mut self) -> bool {
                true
            }
        }
        fn read_fragments(chunks: Vec<Vec<u8>>) -> std::result::Result<Vec<u8>, ureq::Error> {
            let inner = Fragments {
                buffers: LazyBuffers::new(MAX_HEADERS + 32, 32),
                chunks: chunks.into(),
            };
            let mut guard = HeaderTransport {
                inner,
                buffers: LazyBuffers::new(MAX_HEADERS + 1, 32),
                checked: false,
                header_deadline: Instant::now() + Duration::from_secs(2),
            };
            let timeout = NextTimeout {
                after: time::Duration::from_secs(2),
                reason: ureq::Timeout::Global,
            };
            let mut bytes = Vec::new();
            while guard.await_input(timeout)? {
                bytes.extend_from_slice(guard.buffers.input());
                let n = guard.buffers.input().len();
                guard.buffers.input_consume(n);
            }
            Ok(bytes)
        }
        let wire = b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nContent-Encoding: identity\r\n\r\nabc";
        // Every possible split, including each byte of the CRLFCRLF delimiter.
        for split in 1..wire.len() {
            assert_eq!(
                read_fragments(vec![wire[..split].to_vec(), wire[split..].to_vec()]).unwrap(),
                wire
            );
        }
        let prefix = b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nX-Pad: ";
        let suffix = b"\r\n\r\n";
        for header_size in [MAX_HEADERS - 1, MAX_HEADERS, MAX_HEADERS + 1] {
            let mut wire = prefix.to_vec();
            wire.extend(vec![b'x'; header_size - prefix.len() - suffix.len()]);
            wire.extend_from_slice(suffix);
            wire.extend_from_slice(b"abc");
            let cuts = [
                0,
                header_size - 3,
                header_size - 1,
                header_size + 1,
                wire.len(),
            ];
            let chunks = cuts.windows(2).map(|c| wire[c[0]..c[1]].to_vec()).collect();
            let result = read_fragments(chunks);
            if header_size <= MAX_HEADERS {
                assert_eq!(result.unwrap(), wire);
            } else {
                assert!(matches!(
                    result,
                    Err(ureq::Error::LargeResponseHeader(_, _))
                ));
            }
        }
    }
}
