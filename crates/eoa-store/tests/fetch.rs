//! End-to-end download tests against a tiny local HTTP server (no Internet).

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use eoa_store::{FetchRequest, NoProgress, Progress, Store, StoreError, StoreOptions};
use sha2::{Digest, Sha256};

/// Behaviour knobs of the test server.
#[derive(Default)]
struct Behaviour {
    /// Honour `Range` headers (206) or always send the whole body (200).
    ranges: bool,
    /// Answer 404 to everything.
    not_found: bool,
    /// After this many *ranged segment* requests, answer 500 (simulates an outage).
    fail_after_segments: Option<usize>,
}

struct TestServer {
    url: String,
    body_bytes_sent: Arc<AtomicU64>,
    healthy: Arc<AtomicBool>,
}

fn serve(data: Arc<Vec<u8>>, behaviour: Behaviour) -> TestServer {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/file.bin", listener.local_addr().unwrap());
    let body_bytes_sent = Arc::new(AtomicU64::new(0));
    let healthy = Arc::new(AtomicBool::new(true));
    let segments = Arc::new(AtomicUsize::new(0));
    let behaviour = Arc::new(behaviour);
    let (sent, ok) = (body_bytes_sent.clone(), healthy.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let (data, behaviour, sent, ok, segments) = (
                data.clone(),
                behaviour.clone(),
                sent.clone(),
                ok.clone(),
                segments.clone(),
            );
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut range = None;
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap_or(0) > 0 {
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        let (a, b) = value.trim().split_once('-').unwrap();
                        range = Some((a.parse::<usize>().unwrap(), b.parse::<usize>().unwrap()));
                    }
                    line.clear();
                }
                let respond = |stream: &mut std::net::TcpStream,
                               status: &str,
                               headers: String,
                               body: &[u8]| {
                    let head = format!(
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(head.as_bytes());
                    let _ = stream.write_all(body);
                    sent.fetch_add(body.len() as u64, Ordering::SeqCst);
                };
                if behaviour.not_found {
                    return respond(&mut stream, "404 Not Found", String::new(), b"");
                }
                match range {
                    Some((start, end)) if behaviour.ranges => {
                        let is_segment = end > start; // the 0-0 probe is not a segment
                        if is_segment {
                            let n = segments.fetch_add(1, Ordering::SeqCst);
                            if behaviour.fail_after_segments.is_some_and(|max| n >= max)
                                && !ok.load(Ordering::SeqCst)
                            {
                                return respond(
                                    &mut stream,
                                    "500 Internal Server Error",
                                    String::new(),
                                    b"",
                                );
                            }
                        }
                        let end = end.min(data.len() - 1);
                        let headers =
                            format!("Content-Range: bytes {start}-{end}/{}\r\n", data.len());
                        respond(
                            &mut stream,
                            "206 Partial Content",
                            headers,
                            &data[start..=end],
                        );
                    }
                    _ => respond(&mut stream, "200 OK", String::new(), &data),
                }
            });
        }
    });
    TestServer {
        url,
        body_bytes_sent,
        healthy,
    }
}

fn payload(size: usize) -> Arc<Vec<u8>> {
    Arc::new((0..size).map(|i| (i * 31 % 251) as u8).collect())
}

fn sha256(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

fn options() -> StoreOptions {
    StoreOptions {
        connections: 4,
        segment_size: 64 * 1024,
        attempts: 2,
        retry_delay: Duration::from_millis(10),
    }
}

#[derive(Default)]
struct Counter(AtomicU64);
impl Progress for Counter {
    fn advance(&self, bytes: u64) {
        self.0.fetch_add(bytes, Ordering::SeqCst);
    }
    fn retract(&self, bytes: u64) {
        self.0.fetch_sub(bytes, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn segmented_download_is_verified_and_stored() {
    let data = payload(1_000_000);
    let server = serve(
        data.clone(),
        Behaviour {
            ranges: true,
            ..Default::default()
        },
    );
    let dir = tempfile::tempdir().unwrap();
    let store = Store::with_options(dir.path(), options()).unwrap();
    let request = FetchRequest {
        urls: vec![server.url.clone()],
        size: data.len() as u64,
        sha256: sha256(&data),
    };

    let progress = Counter::default();
    let path = store.fetch(&request, &progress).await.unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), *data);
    assert_eq!(progress.0.load(Ordering::SeqCst), data.len() as u64);
    assert!(store.contains(&request.sha256));
    assert!(
        std::fs::read_dir(dir.path().join("partial"))
            .unwrap()
            .next()
            .is_none(),
        "partial dir cleaned"
    );

    // Second fetch is a cache hit: no new bytes over the network.
    let before = server.body_bytes_sent.load(Ordering::SeqCst);
    store.fetch(&request, &NoProgress).await.unwrap();
    assert_eq!(server.body_bytes_sent.load(Ordering::SeqCst), before);
}

#[tokio::test]
async fn single_stream_download_without_range_support() {
    let data = payload(300_000);
    let server = serve(
        data.clone(),
        Behaviour {
            ranges: false,
            ..Default::default()
        },
    );
    let dir = tempfile::tempdir().unwrap();
    let store = Store::with_options(dir.path(), options()).unwrap();
    let request = FetchRequest {
        urls: vec![server.url],
        size: data.len() as u64,
        sha256: sha256(&data),
    };
    let path = store.fetch(&request, &NoProgress).await.unwrap();
    assert_eq!(std::fs::read(path).unwrap(), *data);
}

#[tokio::test]
async fn hash_mismatch_is_rejected_and_cleaned() {
    let data = payload(200_000);
    let server = serve(
        data.clone(),
        Behaviour {
            ranges: true,
            ..Default::default()
        },
    );
    let dir = tempfile::tempdir().unwrap();
    let store = Store::with_options(dir.path(), options()).unwrap();
    let wrong = "0".repeat(64);
    let request = FetchRequest {
        urls: vec![server.url],
        size: data.len() as u64,
        sha256: wrong.clone(),
    };
    let error = store.fetch(&request, &NoProgress).await.unwrap_err();
    assert!(matches!(error, StoreError::HashMismatch { .. }), "{error}");
    assert!(!store.contains(&wrong));
    assert!(
        std::fs::read_dir(dir.path().join("partial"))
            .unwrap()
            .next()
            .is_none(),
        "partial data removed"
    );
}

#[tokio::test]
async fn falls_back_to_the_next_mirror() {
    let data = payload(150_000);
    let broken = serve(
        data.clone(),
        Behaviour {
            not_found: true,
            ..Default::default()
        },
    );
    let good = serve(
        data.clone(),
        Behaviour {
            ranges: true,
            ..Default::default()
        },
    );
    let dir = tempfile::tempdir().unwrap();
    let store = Store::with_options(dir.path(), options()).unwrap();
    let request = FetchRequest {
        urls: vec![broken.url, good.url],
        size: data.len() as u64,
        sha256: sha256(&data),
    };
    let path = store.fetch(&request, &NoProgress).await.unwrap();
    assert_eq!(std::fs::read(path).unwrap(), *data);
}

#[tokio::test]
async fn interrupted_download_resumes_missing_segments_only() {
    let data = payload(2_000_000); // 31 segments of 64 KiB
    let server = serve(
        data.clone(),
        Behaviour {
            ranges: true,
            fail_after_segments: Some(10),
            ..Default::default()
        },
    );
    let dir = tempfile::tempdir().unwrap();
    let store = Store::with_options(dir.path(), options()).unwrap();
    let request = FetchRequest {
        urls: vec![server.url.clone()],
        size: data.len() as u64,
        sha256: sha256(&data),
    };

    // Outage after 10 segments: the fetch fails but keeps completed segments.
    server.healthy.store(false, Ordering::SeqCst);
    assert!(store.fetch(&request, &NoProgress).await.is_err());
    let after_failure = server.body_bytes_sent.load(Ordering::SeqCst);
    assert!(after_failure > 0 && after_failure < data.len() as u64);

    // Network back: only the missing part is transferred.
    server.healthy.store(true, Ordering::SeqCst);
    let progress = Counter::default();
    let path = store.fetch(&request, &progress).await.unwrap();
    assert_eq!(std::fs::read(path).unwrap(), *data);
    let resumed = server.body_bytes_sent.load(Ordering::SeqCst) - after_failure;
    assert!(
        resumed < data.len() as u64,
        "resumed {resumed} bytes, should be less than the full {}",
        data.len()
    );
    assert_eq!(
        progress.0.load(Ordering::SeqCst),
        data.len() as u64,
        "progress counts resumed bytes once"
    );
}

#[tokio::test]
async fn size_mismatch_reported_by_server_fails_fast() {
    let data = payload(500_000);
    let server = serve(
        data.clone(),
        Behaviour {
            ranges: true,
            ..Default::default()
        },
    );
    let dir = tempfile::tempdir().unwrap();
    let store = Store::with_options(dir.path(), options()).unwrap();
    let request = FetchRequest {
        urls: vec![server.url],
        size: 999_999,
        sha256: sha256(&data),
    };
    let error = store.fetch(&request, &NoProgress).await.unwrap_err();
    assert!(error.to_string().contains("size"), "{error}");
}
