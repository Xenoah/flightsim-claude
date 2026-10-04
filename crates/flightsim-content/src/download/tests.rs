use std::{
    collections::VecDeque,
    fs,
    io::{self, Cursor},
    net::IpAddr,
    time::Duration,
};

use super::*;
use ureq::http::{HeaderMap, HeaderValue};

const ZIP: &[u8] = include_bytes!("../../tests/data/download-fixture.zip");
const RELEASE: &str = "https://github.com/example/terrain/releases/download/v1/area.zip";
const RAW: &str = "https://raw.githubusercontent.com/example/terrain/0123456789012345678901234567890123456789/regions/area.zip";
fn source() -> DownloadSource {
    DownloadSource::github(RELEASE, &crate::sha256(ZIP)).unwrap()
}
fn response(status: u16, headers: &[(&str, &str)], bytes: &[u8]) -> http::Response {
    let mut map = HeaderMap::new();
    for (key, val) in headers {
        map.append(
            ureq::http::HeaderName::from_bytes(key.as_bytes()).unwrap(),
            HeaderValue::from_str(val).unwrap(),
        );
    }
    http::Response {
        status,
        headers: map,
        body: Box::new(Cursor::new(bytes.to_vec())),
    }
}
#[derive(Default)]
struct MockHttp {
    responses: VecDeque<Result<http::Response>>,
    urls: Vec<String>,
}
impl MockHttp {
    fn zip() -> Self {
        Self {
            responses: VecDeque::from([Ok(response(200, &[], ZIP))]),
            ..Self::default()
        }
    }
    fn one(response: http::Response) -> Self {
        Self {
            responses: VecDeque::from([Ok(response)]),
            ..Self::default()
        }
    }
}
impl Http for MockHttp {
    fn get(&mut self, url: &url::Url, remaining: Duration) -> Result<http::Response> {
        assert!(remaining > Duration::ZERO && remaining <= NETWORK_TIMEOUT);
        self.urls.push(url.to_string());
        self.responses.pop_front().expect("unexpected network call")
    }
}
fn clean(root: &Path) {
    for name in ["cache", "store"] {
        let dir = root.join(name);
        if dir.exists() {
            assert!(!fs::read_dir(dir).unwrap().any(|e| {
                let e = e.unwrap();
                let name = e.file_name().to_string_lossy().into_owned();
                name.starts_with(".download-") || name.starts_with(".import-")
            }));
        }
    }
}
fn stage(root: &Path, http: &mut MockHttp) -> Result<StagedDownload> {
    stage_with_http(
        &source(),
        &root.join("cache"),
        &root.join("store"),
        CacheMode::PreferCache,
        |_| true,
        http,
    )
}

#[test]
fn source_contract_accepts_only_explicit_hash_pinned_prepared_github_urls() {
    let digest = crate::sha256(ZIP);
    assert!(DownloadSource::github(RELEASE, &digest).is_ok());
    assert!(DownloadSource::github(RAW, &digest).is_ok());
    let invalid = [
        "http://github.com/example/terrain/releases/download/v1/area.zip",
        "https://github.com.evil.test/example/terrain/releases/download/v1/area.zip",
        "https://user@github.com/example/terrain/releases/download/v1/area.zip",
        "https://github.com:444/example/terrain/releases/download/v1/area.zip",
        "https://github.com:443/example/terrain/releases/download/v1/area.zip",
        "https://github.com/example/terrain/releases/latest/download/area.zip",
        "https://github.com/example/terrain/archive/refs/heads/main.zip",
        "https://github.com/example/terrain/releases/download/v1/area.zip?token=secret",
        "https://github.com/example/terrain/releases/download/v1/area.zip#fragment",
        "https://github.com/example/terrain/releases/download/v1/../area.zip",
        "https://github.com/example/terrain/releases/download/v1/%2e%2e.zip",
        "https://github.com/example/terrain/releases/download/v1/area.tif",
        "https://raw.githubusercontent.com/example/terrain/main/area.zip",
        "https://127.0.0.1/example/terrain/releases/download/v1/area.zip",
        "https://[::1]/example/terrain/releases/download/v1/area.zip",
        "https://github.com/example\\terrain/releases/download/v1/area.zip",
    ];
    for url in invalid {
        assert!(DownloadSource::github(url, &digest).is_err(), "{url}");
    }
    for hash in [
        "",
        "sha256:123",
        &digest.to_uppercase(),
        &"0".repeat(63),
        &"g".repeat(64),
    ] {
        assert!(DownloadSource::github(RELEASE, hash).is_err());
    }
}

#[test]
fn redirect_policy_does_not_promote_cdn_urls_to_sources_or_allow_suffix_tricks() {
    let source = source();
    for host in [
        "release-assets.githubusercontent.com",
        "github-releases.githubusercontent.com",
        "objects.githubusercontent.com",
    ] {
        let url = format!("https://{host}/asset?sig=temporary");
        assert!(source.redirect(&url).is_ok());
        assert!(DownloadSource::github(&url, source.archive_sha256()).is_err());
    }
    for bad in [
        "https://evil.test/area.zip",
        "http://objects.githubusercontent.com/asset",
        "https://objects.githubusercontent.com.evil.test/asset",
        "https://127.0.0.1/a",
        "https://localhost/a",
        "file:///tmp/a",
        "/another/asset",
        "//objects.githubusercontent.com/asset",
        "https://secret@objects.githubusercontent.com/asset",
        "https://objects.githubusercontent.com:444/asset",
        "https://github.com/other/repo/releases/download/v1/area.zip",
    ] {
        assert!(source.redirect(bad).is_err(), "{bad}");
    }
    assert!(
        source
            .redirect(&format!(
                "https://objects.githubusercontent.com/{}",
                "x".repeat(8192)
            ))
            .is_err()
    );
    assert!(
        DownloadSource::github(RAW, source.archive_sha256())
            .unwrap()
            .redirect("https://objects.githubusercontent.com/a")
            .is_err()
    );
}

#[test]
fn reserved_and_mapped_addresses_are_rejected_before_connect() {
    let private = [
        "0.0.0.0",
        "10.1.2.3",
        "100.64.0.1",
        "100.127.255.255",
        "127.0.0.1",
        "169.254.169.254",
        "172.16.0.1",
        "172.31.255.254",
        "192.0.0.9",
        "192.0.2.1",
        "192.168.1.1",
        "192.88.99.1",
        "198.18.0.1",
        "198.19.0.1",
        "198.51.100.1",
        "203.0.113.1",
        "224.0.0.1",
        "240.0.0.1",
        "255.255.255.255",
        "::",
        "::1",
        "::ffff:8.8.8.8",
        "64:ff9b::0808:0808",
        "100::1",
        "fc00::1",
        "fe80::1",
        "ff02::1",
        "2001:db8::1",
        "2001::1",
        "2002:0808:0808::1",
        "3fff::1",
    ];
    for ip in private {
        assert!(
            !source::public_address(ip.parse::<IpAddr>().unwrap()),
            "{ip}"
        );
    }
    for ip in ["140.82.112.3", "185.199.108.133", "2606:50c0:8000::154"] {
        assert!(source::public_address(ip.parse().unwrap()), "{ip}");
    }
}

#[test]
fn download_cache_offline_and_explicit_install_retain_source_and_manifest_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let mut http = MockHttp::zip();
    let mut events = Vec::new();
    let downloaded = stage_with_http(
        &source(),
        &tmp.path().join("cache"),
        &tmp.path().join("store"),
        CacheMode::PreferCache,
        |p| {
            events.push(p);
            true
        },
        &mut http,
    )
    .unwrap();
    assert!(!downloaded.cache_hit);
    assert!(
        crate::list_installed(&tmp.path().join("store"))
            .unwrap()
            .is_empty()
    );
    assert_eq!(downloaded.source, source());
    let identity = downloaded.staged.identity().clone();
    assert!(events.iter().any(|p| matches!(p, DownloadProgress::Receiving { bytes_done, .. } if *bytes_done == ZIP.len() as u64)));
    drop(downloaded);
    let mut no_http = MockHttp::default();
    let reused = stage_with_http(
        &source(),
        &tmp.path().join("cache"),
        &tmp.path().join("store"),
        CacheMode::Offline,
        |_| true,
        &mut no_http,
    )
    .unwrap();
    assert!(reused.cache_hit);
    assert_eq!(reused.staged.identity(), &identity);
    let installed = reused.staged.commit().unwrap();
    assert_eq!(
        crate::inspect_installed(installed.directory())
            .unwrap()
            .identity(),
        &identity
    );
    assert_eq!(http.urls, [RELEASE]);
    assert!(no_http.urls.is_empty());
    clean(tmp.path());
}

#[test]
fn offline_miss_never_constructs_a_request_or_creates_store() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(matches!(
        stage_with_http(
            &source(),
            &tmp.path().join("cache"),
            &tmp.path().join("store"),
            CacheMode::Offline,
            |_| true,
            &mut MockHttp::default()
        ),
        Err(DownloadError::CacheMiss)
    ));
    assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 0);
}

#[test]
fn corrupt_cached_bytes_and_receipts_fail_closed_without_network() {
    for what in ["bytes", "receipt", "extra", "manifest"] {
        let tmp = tempfile::tempdir().unwrap();
        drop(stage(tmp.path(), &mut MockHttp::zip()).unwrap());
        let entry = tmp.path().join("cache").join(source().key());
        match what {
            "bytes" => fs::write(entry.join(PACKAGE), b"corrupt").unwrap(),
            "receipt" => fs::write(entry.join(RECEIPT), b"{}").unwrap(),
            "extra" => fs::write(entry.join("extra"), b"ignored?").unwrap(),
            "manifest" => {
                let path = entry.join(RECEIPT);
                let mut receipt: Receipt =
                    serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                receipt.manifest_sha256 = "0".repeat(64);
                fs::write(path, serde_json::to_vec(&receipt).unwrap()).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            stage(tmp.path(), &mut MockHttp::default()).is_err(),
            "{what}"
        );
        clean(tmp.path());
    }
}

#[test]
fn cancellation_at_every_observable_stage_cleans_only_own_temporary_state() {
    for phase in 0..5 {
        let tmp = tempfile::tempdir().unwrap();
        let mut http = MockHttp::zip();
        let result = stage_with_http(
            &source(),
            &tmp.path().join("cache"),
            &tmp.path().join("store"),
            CacheMode::PreferCache,
            |p| {
                !matches!(
                    (phase, p),
                    (0, DownloadProgress::CheckingCache)
                        | (1, DownloadProgress::Connecting { .. })
                        | (2, DownloadProgress::Receiving { .. })
                        | (3, DownloadProgress::Importing(_))
                        | (4, DownloadProgress::Ready { .. })
                )
            },
            &mut http,
        );
        assert!(matches!(result, Err(DownloadError::Cancelled)), "{phase}");
        assert!(!tmp.path().join("cache").join(source().key()).exists());
        assert!(
            crate::list_installed(&tmp.path().join("store"))
                .unwrap()
                .is_empty()
        );
        clean(tmp.path());
        // Retrying after cancellation releases the cache lock and starts cleanly.
        drop(stage(tmp.path(), &mut MockHttp::zip()).unwrap());
    }
    let tmp = tempfile::tempdir().unwrap();
    drop(stage(tmp.path(), &mut MockHttp::zip()).unwrap());
    assert!(matches!(
        stage_with_http(
            &source(),
            &tmp.path().join("cache"),
            &tmp.path().join("store"),
            CacheMode::Offline,
            |p| !matches!(p, DownloadProgress::VerifyingCache { .. }),
            &mut MockHttp::default()
        ),
        Err(DownloadError::Cancelled)
    ));
    assert!(tmp.path().join("cache").join(source().key()).is_dir());
    clean(tmp.path());
}

#[test]
fn cache_job_lock_is_nonblocking_platform_correct_and_separate_from_install() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    fs::create_dir(&cache).unwrap();
    let held = cache_lock(&cache).unwrap();
    assert!(matches!(
        stage(tmp.path(), &mut MockHttp::default()),
        Err(DownloadError::CacheBusy)
    ));
    let zip = tmp.path().join("input.zip");
    fs::write(&zip, ZIP).unwrap();
    let installed = crate::install_zip(&zip, &tmp.path().join("store")).unwrap();
    assert_eq!(installed.identity().id, "org.example.download");
    drop(held);
    drop(stage(tmp.path(), &mut MockHttp::zip()).unwrap());
    clean(tmp.path());
}

#[test]
fn hash_mismatch_invalid_zip_and_failed_requests_never_publish_cache() {
    for kind in 0..4 {
        let tmp = tempfile::tempdir().unwrap();
        let mut http = match kind {
            0 => MockHttp::one(response(200, &[], b"wrong bytes")),
            1 => MockHttp::one(response(404, &[], b"not found")),
            2 => MockHttp {
                responses: VecDeque::from([Err(DownloadError::Network("timeout"))]),
                ..MockHttp::default()
            },
            _ => MockHttp::one(response(200, &[], b"not a zip")),
        };
        let request = if kind == 3 {
            DownloadSource::github(RELEASE, &crate::sha256(b"not a zip")).unwrap()
        } else {
            source()
        };
        assert!(
            stage_with_http(
                &request,
                &tmp.path().join("cache"),
                &tmp.path().join("store"),
                CacheMode::PreferCache,
                |_| true,
                &mut http
            )
            .is_err()
        );
        assert!(!tmp.path().join("cache").join(request.key()).exists());
        clean(tmp.path());
        drop(stage(tmp.path(), &mut MockHttp::zip()).unwrap());
    }
}

#[test]
fn redirects_are_checked_before_requests_and_progress_never_contains_signed_urls() {
    let tmp = tempfile::tempdir().unwrap();
    let cdn = "https://release-assets.githubusercontent.com/asset?sig=temporary";
    let mut http = MockHttp {
        responses: VecDeque::from([
            Ok(response(302, &[("location", cdn)], &[])),
            Ok(response(200, &[], ZIP)),
        ]),
        ..MockHttp::default()
    };
    drop(stage(tmp.path(), &mut http).unwrap());
    assert_eq!(http.urls, [RELEASE, cdn]);
    let receipt =
        fs::read_to_string(tmp.path().join("cache").join(source().key()).join(RECEIPT)).unwrap();
    assert!(!receipt.contains("temporary"));
    for target in [
        "https://127.0.0.1/secret",
        "https://evil.test/area.zip",
        RELEASE,
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let mut http = MockHttp::one(response(302, &[("location", target)], &[]));
        assert!(stage(tmp.path(), &mut http).is_err());
        assert_eq!(http.urls.len(), 1);
        clean(tmp.path());
    }
    let tmp = tempfile::tempdir().unwrap();
    let mut http = MockHttp::default();
    for suffix in ["a", "b", "c", "d"] {
        http.responses.push_back(Ok(response(
            302,
            &[("location", &format!("{cdn}{suffix}"))],
            &[],
        )));
    }
    assert!(matches!(
        stage(tmp.path(), &mut http),
        Err(DownloadError::Limit("redirect count"))
    ));
    assert_eq!(http.urls.len(), 4);
}

#[test]
fn byte_caps_ignore_untrusted_length_claims_and_never_allocate_the_claimed_body() {
    for headers in [
        vec![("content-length", "536870913")],
        vec![("content-length", "1")],
        vec![("content-length", "9999")],
        vec![("content-length", "1"), ("content-length", "1")],
        vec![("content-length", "1"), ("transfer-encoding", "chunked")],
        vec![("content-length", "-1")],
        vec![("content-encoding", "gzip")],
        vec![("content-type", "text/plain; charset=iso-8859-1")],
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let mut http = MockHttp::one(response(200, &headers, ZIP));
        assert!(stage(tmp.path(), &mut http).is_err(), "{headers:?}");
        clean(tmp.path());
    }
    // Infinite unknown-length reader gets exactly limit+1 bytes requested. Tests
    // use a small limit to exercise the production loop without huge disk I/O.
    let tmp = tempfile::tempdir().unwrap();
    let mut http = MockHttp::one(http::Response {
        status: 200,
        headers: HeaderMap::new(),
        body: Box::new(io::repeat(0)),
    });
    assert!(matches!(
        receive(
            &source(),
            &tmp.path().join("out"),
            3,
            &mut |_| true,
            &mut http
        ),
        Err(DownloadError::Limit(_))
    ));
    assert_eq!(fs::metadata(tmp.path().join("out")).unwrap().len(), 0);
}

#[test]
fn abandoned_job_dirs_are_ignored_but_not_deleted_and_root_entries_are_bounded() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    fs::create_dir_all(cache.join(".download-crashed")).unwrap();
    fs::write(
        cache.join(".download-crashed/keep"),
        b"owned by another job",
    )
    .unwrap();
    drop(stage(tmp.path(), &mut MockHttp::zip()).unwrap());
    assert!(cache.join(".download-crashed/keep").exists());
    for i in 0..MAX_CACHE_ROOT_ENTRIES {
        fs::create_dir(cache.join(format!(".download-{i}"))).unwrap();
    }
    assert!(matches!(
        stage(tmp.path(), &mut MockHttp::default()),
        Err(DownloadError::Limit("cache root entries"))
    ));
}

#[cfg(unix)]
#[test]
fn symlinked_cache_root_lock_entry_and_payload_are_rejected() {
    use std::os::unix::fs::symlink;
    for kind in 0..4 {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tmp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        let cache = tmp.path().join("cache");
        if kind == 0 {
            symlink(&outside, &cache).unwrap();
        } else {
            fs::create_dir(&cache).unwrap();
            if kind == 1 {
                let file = outside.join("lock");
                fs::write(&file, b"").unwrap();
                symlink(file, cache.join(".download.lock")).unwrap();
            }
            if kind == 2 {
                symlink(&outside, cache.join(source().key())).unwrap();
            }
            if kind == 3 {
                drop(stage(tmp.path(), &mut MockHttp::zip()).unwrap());
                let payload = cache.join(source().key()).join(PACKAGE);
                fs::remove_file(&payload).unwrap();
                let file = outside.join(PACKAGE);
                fs::write(&file, ZIP).unwrap();
                symlink(file, payload).unwrap();
            }
        }
        assert!(stage(tmp.path(), &mut MockHttp::default()).is_err());
    }
}

#[test]
fn cancellation_after_received_bytes_removes_partial_archive_and_allows_retry() {
    let tmp = tempfile::tempdir().unwrap();
    let mut received = false;
    let result = stage_with_http(
        &source(),
        &tmp.path().join("cache"),
        &tmp.path().join("store"),
        CacheMode::PreferCache,
        |p| {
            if matches!(p, DownloadProgress::Receiving { bytes_done, .. } if bytes_done > 0) {
                received = true;
                false
            } else {
                true
            }
        },
        &mut MockHttp::zip(),
    );
    assert!(received);
    assert!(matches!(result, Err(DownloadError::Cancelled)));
    assert!(!tmp.path().join("cache").join(source().key()).exists());
    clean(tmp.path());
    drop(stage(tmp.path(), &mut MockHttp::zip()).unwrap());
}

#[test]
fn completed_cache_bytes_and_entries_are_bounded_without_eviction() {
    for bytes_limit in [true, false] {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tmp.path().join("cache");
        fs::create_dir(&cache).unwrap();
        let count = if bytes_limit { 4 } else { MAX_CACHE_ENTRIES };
        for i in 0..count {
            let path = cache.join(format!("{i:064x}"));
            fs::create_dir(&path).unwrap();
            let file = File::create(path.join(PACKAGE)).unwrap();
            file.set_len(if bytes_limit { MAX_ARCHIVE_BYTES } else { 0 })
                .unwrap();
        }
        assert!(matches!(
            stage(tmp.path(), &mut MockHttp::default()),
            Err(DownloadError::Limit(_))
        ));
        assert_eq!(
            fs::read_dir(&cache)
                .unwrap()
                .filter(|e| !e
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with('.'))
                .count(),
            count
        );
        clean(tmp.path());
    }
}

#[test]
fn same_url_with_changed_hash_cannot_replace_verified_cache_or_installed_version() {
    let tmp = tempfile::tempdir().unwrap();
    let downloaded = stage(tmp.path(), &mut MockHttp::zip()).unwrap();
    let installed = downloaded.staged.commit().unwrap();
    let original_identity = installed.identity().clone();
    let wrong = DownloadSource::github(RELEASE, &"0".repeat(64)).unwrap();
    assert_ne!(source().key(), wrong.key());
    assert!(matches!(
        stage_with_http(
            &wrong,
            &tmp.path().join("cache"),
            &tmp.path().join("store"),
            CacheMode::PreferCache,
            |_| true,
            &mut MockHttp::zip()
        ),
        Err(DownloadError::HashMismatch)
    ));
    assert_eq!(
        crate::inspect_installed(installed.directory())
            .unwrap()
            .identity(),
        &original_identity
    );
    assert_eq!(
        fs::read(tmp.path().join("cache").join(source().key()).join(PACKAGE)).unwrap(),
        ZIP
    );
    assert!(!tmp.path().join("cache").join(wrong.key()).exists());
    clean(tmp.path());
}

#[test]
fn overlapping_cache_and_install_store_roots_fail_before_network_or_job_lock() {
    for nested in 0..3 {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tmp.path().join("cache");
        let store = match nested {
            0 => cache.clone(),
            1 => cache.join("store"),
            _ => tmp.path().to_path_buf(),
        };
        assert!(matches!(
            stage_with_http(
                &source(),
                &cache,
                &store,
                CacheMode::PreferCache,
                |_| true,
                &mut MockHttp::default()
            ),
            Err(DownloadError::InvalidSource(_))
        ));
        assert!(!cache.join(".download.lock").exists());
    }
}
