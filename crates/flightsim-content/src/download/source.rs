use std::net::IpAddr;

use url::Url;

use super::{DownloadError, Result};

const MAX_SOURCE_URL: usize = 2048;
pub(super) const MAX_REDIRECT_URL: usize = 8192;

/// An explicit public GitHub prepared ZIP and the independently supplied hash of
/// its exact archive bytes. Neither field establishes the provider's data rights.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadSource {
    url: Url,
    archive_sha256: String,
}

impl DownloadSource {
    /// Accept a release asset URL or commit-pinned raw URL. Mutable raw branches,
    /// latest-release aliases, repository archives, URL credentials and queries
    /// are deliberately unsupported. See docs/content-downloads.md.
    pub fn github(url: &str, archive_sha256: &str) -> Result<Self> {
        if !is_sha256(archive_sha256) {
            return Err(DownloadError::InvalidSource("expected lowercase SHA-256"));
        }
        let parsed = parse_https(url, MAX_SOURCE_URL)?;
        // Reject normalization aliases, including dot segments and default ports.
        if parsed.as_str() != url || parsed.query().is_some() {
            return Err(DownloadError::InvalidSource("noncanonical source URL"));
        }
        let segments: Vec<_> = parsed.path().split('/').skip(1).collect();
        if segments.iter().any(|s| !component(s)) || segments.len() > 16 {
            return Err(DownloadError::InvalidSource("unsupported URL path"));
        }
        let valid = match parsed.host_str() {
            Some("github.com") => {
                segments.len() == 6
                    && segments[2..4] == ["releases", "download"]
                    && segments[4] != "latest"
            }
            Some("raw.githubusercontent.com") => {
                segments.len() >= 4
                    && segments[2].len() == 40
                    && segments[2]
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            }
            _ => false,
        };
        if !valid || !segments.last().is_some_and(|s| s.ends_with(".zip")) {
            return Err(DownloadError::InvalidSource(
                "expected public GitHub prepared .zip URL",
            ));
        }
        Ok(Self {
            url: parsed,
            archive_sha256: archive_sha256.into(),
        })
    }

    pub fn url(&self) -> &str {
        self.url.as_str()
    }
    pub fn archive_sha256(&self) -> &str {
        &self.archive_sha256
    }

    pub(super) fn key(&self) -> String {
        crate::sha256(format!("{}\n{}", self.url, self.archive_sha256).as_bytes())
    }

    pub(super) fn redirect(&self, location: &str) -> Result<Url> {
        // GitHub supplies absolute signed CDN URLs. Do not widen this to generic
        // URL joining, arbitrary hosts or user-provided CDN entry points.
        let url = parse_https(location, MAX_REDIRECT_URL)?;
        let same_source = url == self.url;
        let release_cdn = self.url.host_str() == Some("github.com")
            && matches!(
                url.host_str(),
                Some(
                    "release-assets.githubusercontent.com"
                        | "github-releases.githubusercontent.com"
                        | "objects.githubusercontent.com"
                )
            );
        if !same_source && !release_cdn {
            return Err(DownloadError::InvalidSource(
                "redirect host/source is not allowed",
            ));
        }
        Ok(url)
    }
}

fn component(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 255
        && s != "."
        && s != ".."
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

pub(super) fn is_sha256(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn parse_https(value: &str, max: usize) -> Result<Url> {
    if value.len() > max
        || !value.is_ascii()
        || value.bytes().any(|b| b <= 32 || b == 127 || b == b'\\')
    {
        return Err(DownloadError::InvalidSource("invalid URL envelope"));
    }
    let url = Url::parse(value).map_err(|_| DownloadError::InvalidSource("invalid URL"))?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.fragment().is_some()
    {
        return Err(DownloadError::InvalidSource(
            "HTTPS without credentials, port or fragment required",
        ));
    }
    Ok(url)
}

pub(super) fn allowed_host(host: &str) -> bool {
    matches!(
        host,
        "github.com"
            | "raw.githubusercontent.com"
            | "release-assets.githubusercontent.com"
            | "github-releases.githubusercontent.com"
            | "objects.githubusercontent.com"
    )
}

// Conservative routable unicast allowlist. Reject special-purpose ranges rather
// than trying to distinguish the few globally reachable exceptions within them.
// No IPv4-mapped, NAT64, local-use, multicast or tunnelling IPv6 is accepted.
pub(super) fn public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(a == 0
                || a == 10
                || a == 127
                || a >= 224
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192
                    && (b == 168 || (b == 0 && (c == 0 || c == 2)) || (b == 88 && c == 99)))
                || (a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
                || (a == 203 && b == 0 && c == 113))
        }
        IpAddr::V6(ip) => {
            let s = ip.segments();
            (s[0] & 0xe000) == 0x2000
                && !(s[0] == 0x2001 && (s[1] < 0x0200 || s[1] == 0x0db8))
                && s[0] != 0x2002
                && !(s[0] == 0x3fff && s[1] < 0x1000)
        }
    }
}
