use std::{
    collections::HashMap,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use super::{VideoDecoder, probe_video};
use crate::audio::AudioPlayer;

type Routes = HashMap<String, (String, Vec<u8>)>;

struct EncryptedHlsFixture {
    directory: PathBuf,
    playlist: Vec<u8>,
    key: Vec<u8>,
    segment: Vec<u8>,
}

struct TestHttpServer {
    address: SocketAddr,
    stopping: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl TestHttpServer {
    fn spawn(routes: Routes) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test HTTP listener should bind");
        listener
            .set_nonblocking(true)
            .expect("test HTTP listener should become nonblocking");
        let address = listener
            .local_addr()
            .expect("test HTTP listener should have an address");
        let stopping = Arc::new(AtomicBool::new(false));
        let thread_stopping = Arc::clone(&stopping);
        let thread = thread::spawn(move || {
            while !thread_stopping.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => serve_request(&mut stream, &routes),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            address,
            stopping,
            thread: Some(thread),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.address)
    }
}

impl Drop for TestHttpServer {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        let _ = TcpStream::connect(self.address);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct StallingHttpServer(TestHttpServer);

impl StallingHttpServer {
    fn spawn() -> Self {
        let listener =
            TcpListener::bind("127.0.0.1:0").expect("stalling HTTP listener should bind");
        let address = listener
            .local_addr()
            .expect("stalling HTTP listener should have an address");
        let stopping = Arc::new(AtomicBool::new(false));
        let thread_stopping = Arc::clone(&stopping);
        let thread = thread::spawn(move || {
            if let Ok((_stream, _)) = listener.accept() {
                while !thread_stopping.load(Ordering::Acquire) {
                    thread::sleep(Duration::from_millis(2));
                }
            }
        });
        Self(TestHttpServer {
            address,
            stopping,
            thread: Some(thread),
        })
    }

    fn url(&self) -> String {
        self.0.url("/stall")
    }
}

fn serve_request(stream: &mut TcpStream, routes: &Routes) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    while request.len() < 16 * 1024 && !request.windows(4).any(|part| part == b"\r\n\r\n") {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => request.extend_from_slice(&buffer[..read]),
        }
    }
    let request = String::from_utf8_lossy(&request);
    let target = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(request_path)
        .unwrap_or("/");

    if let Some((content_type, body)) = routes.get(target) {
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(header.as_bytes());
        let _ = stream.write_all(body);
    } else {
        let _ = stream
            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    }
}

fn request_path(target: &str) -> Option<&str> {
    if target.starts_with('/') {
        return Some(target);
    }
    let (_, after_scheme) = target.split_once("://")?;
    Some(
        after_scheme
            .find('/')
            .map_or("/", |index| &after_scheme[index..]),
    )
}

fn generated_mpeg_ts(label: &str, duration_secs: u64) -> Option<(PathBuf, Vec<u8>)> {
    if Command::new("ffmpeg").arg("-version").output().is_err() {
        return None;
    }
    let path = temp_path(label).with_extension("ts");
    let status = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("color=size=16x16:duration={duration_secs}:rate=25"))
        .args(["-c:v", "mpeg2video", "-f", "mpegts"])
        .arg(&path)
        .status()
        .expect("ffmpeg should generate the MPEG-TS fixture");
    assert!(
        status.success(),
        "ffmpeg should generate the MPEG-TS fixture"
    );
    let bytes = std::fs::read(&path).expect("MPEG-TS fixture should be readable");
    Some((path, bytes))
}

fn generated_encrypted_hls(label: &str) -> Option<EncryptedHlsFixture> {
    if Command::new("ffmpeg").arg("-version").output().is_err() {
        return None;
    }
    let directory = temp_path(label);
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir(&directory).expect("encrypted HLS fixture directory should be created");
    let key_path = directory.join("key.ts");
    let key = (0_u8..16).collect::<Vec<_>>();
    std::fs::write(&key_path, &key).expect("encrypted HLS key should be written");
    let key_info_path = directory.join("key-info.txt");
    std::fs::write(&key_info_path, format!("key.ts\n{}\n", key_path.display()))
        .expect("encrypted HLS key info should be written");
    let segment_template = directory.join("segment%d.ts");
    let segment_path = directory.join("segment0.ts");
    let playlist_path = directory.join("index.m3u8");
    let status = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("color=size=16x16:duration=1:rate=25")
        .args(["-c:v", "mpeg2video", "-f", "hls", "-hls_time", "1"])
        .args(["-hls_list_size", "0", "-hls_key_info_file"])
        .arg(&key_info_path)
        .arg("-hls_segment_filename")
        .arg(&segment_template)
        .arg(&playlist_path)
        .status()
        .expect("ffmpeg should generate the encrypted HLS fixture");
    assert!(
        status.success(),
        "ffmpeg should generate the encrypted HLS fixture"
    );
    let playlist = std::fs::read(playlist_path).expect("encrypted HLS playlist should be readable");
    let segment = std::fs::read(segment_path).expect("encrypted HLS segment should be readable");
    Some(EncryptedHlsFixture {
        directory,
        playlist,
        key,
        segment,
    })
}

fn temp_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "enzo-{label}-{}-{}",
        std::process::id(),
        thread::current().name().unwrap_or("test")
    ))
}

#[test]
fn remote_hls_allows_cross_origin_nonstandard_segments() {
    let Some((segment_path, segment)) = generated_mpeg_ts("remote-hls-segment", 8) else {
        return;
    };
    let segment_server = TestHttpServer::spawn(HashMap::from([(
        "/segment.xls".to_string(),
        ("video/mp2t".to_string(), segment),
    )]));
    let segment_url = segment_server.url("/segment.xls");
    let playlist = format!(
        "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:8\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:8,\n{segment_url}\n#EXT-X-ENDLIST\n"
    );
    let playlist_server = TestHttpServer::spawn(HashMap::from([(
        "/index.m3u8".to_string(),
        (
            "application/vnd.apple.mpegurl".to_string(),
            playlist.into_bytes(),
        ),
    )]));

    let info = probe_video(Path::new(&playlist_server.url("/index.m3u8")))
        .expect("cross-origin HLS segment should be allowed");
    assert_eq!((info.width, info.height), (16, 16));
    assert!(info.seekable, "finite HLS media should be seekable");

    let mut decoder = VideoDecoder::spawn_at(
        Path::new(&playlist_server.url("/index.m3u8")),
        16,
        16,
        info.fps,
        Duration::from_secs(6),
        true,
    )
    .expect("finite HLS media should start at a saved position");
    let generation = decoder.seek_generation();
    let deadline = Instant::now() + Duration::from_secs(3);
    let pts = loop {
        if let Some(pts) = decoder.seek_frame(generation) {
            break pts;
        }
        assert!(
            Instant::now() < deadline,
            "finite HLS seek frame should become ready"
        );
        thread::sleep(Duration::from_millis(2));
    };
    assert!(pts >= Duration::from_millis(5_950));
    decoder.stop().expect("video decoder should stop");

    let _ = std::fs::remove_file(segment_path);
}

#[test]
fn remote_live_hls_is_not_seekable() {
    let Some((segment_path, segment)) = generated_mpeg_ts("remote-live-hls-segment", 1) else {
        return;
    };
    let playlist = b"#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:1,\nsegment.ts\n";
    let server = TestHttpServer::spawn(HashMap::from([
        (
            "/index.m3u8".to_string(),
            (
                "application/vnd.apple.mpegurl".to_string(),
                playlist.to_vec(),
            ),
        ),
        (
            "/segment.ts".to_string(),
            ("video/mp2t".to_string(), segment),
        ),
    ]));

    let info =
        probe_video(Path::new(&server.url("/index.m3u8"))).expect("live HLS media should open");
    assert!(!info.seekable, "live HLS media should not be seekable");

    let _ = std::fs::remove_file(segment_path);
}

#[test]
fn remote_hls_cannot_open_local_file_segments() {
    let Some((segment_path, _)) = generated_mpeg_ts("blocked-local-hls-segment", 1) else {
        return;
    };
    let local_segment_url = format!("file://{}", segment_path.display());
    let playlist = format!(
        "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:1,\n{local_segment_url}\n#EXT-X-ENDLIST\n"
    );
    let server = TestHttpServer::spawn(HashMap::from([(
        "/index.m3u8".to_string(),
        (
            "application/vnd.apple.mpegurl".to_string(),
            playlist.into_bytes(),
        ),
    )]));

    let result = probe_video(Path::new(&server.url("/index.m3u8")));
    assert!(result.is_err(), "remote HLS must not read a local segment");

    let _ = std::fs::remove_file(segment_path);
}

#[test]
fn native_input_rejects_unsafe_top_level_protocols() {
    let Some((segment_path, _)) = generated_mpeg_ts("blocked-top-level-protocol", 1) else {
        return;
    };
    for input in [
        format!("concat:{}", segment_path.display()),
        "data:text/plain,not-media".to_string(),
        "lavfi:testsrc".to_string(),
        format!("crypto:{}", segment_path.display()),
    ] {
        assert!(
            probe_video(Path::new(&input)).is_err(),
            "native input must reject {input}"
        );
    }

    let _ = std::fs::remove_file(segment_path);
}

#[test]
fn encrypted_hls_supports_local_and_remote_references() {
    let Some(fixture) = generated_encrypted_hls("encrypted-hls") else {
        return;
    };
    let local_info = probe_video(&fixture.directory.join("index.m3u8"))
        .expect("local encrypted HLS references should be allowed");
    assert_eq!((local_info.width, local_info.height), (16, 16));

    let server = TestHttpServer::spawn(HashMap::from([
        (
            "/index.m3u8".to_string(),
            (
                "application/vnd.apple.mpegurl".to_string(),
                fixture.playlist,
            ),
        ),
        (
            "/key.ts".to_string(),
            ("application/octet-stream".to_string(), fixture.key),
        ),
        (
            "/segment0.ts".to_string(),
            ("video/mp2t".to_string(), fixture.segment),
        ),
    ]));
    let remote_info = probe_video(Path::new(&server.url("/index.m3u8")))
        .expect("remote encrypted HLS should use the approved crypto wrapper");
    assert_eq!((remote_info.width, remote_info.height), (16, 16));

    let _ = std::fs::remove_dir_all(fixture.directory);
}

#[test]
fn stalled_network_open_is_interruptible() {
    let server = StallingHttpServer::spawn();
    let mut audio = AudioPlayer::spawn_held_at(
        Path::new(&server.url()),
        None,
        Duration::ZERO,
        true,
        false,
        100,
    )
    .expect("audio thread should start");
    thread::sleep(Duration::from_millis(100));

    let started = Instant::now();
    let _ = audio.stop();
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "stalled FFmpeg I/O should stop promptly"
    );
}
