use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};

pub(super) struct Server {
    pub(super) endpoint: String,
    address: std::net::SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    pub(super) fn new() -> Self {
        Self::with_description("Inspect project changes")
    }

    pub(super) fn with_description(description: &str) -> Self {
        let commit = "a".repeat(40);
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for name in ["analysis", "review"] {
            zip.start_file(
                format!("skills-{commit}/skills/{name}/SKILL.md"),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            zip.write_all(format!("---\nname: {name}\ndescription: {description}\n---\nRead relevant project files\n").as_bytes()).unwrap();
        }
        let bytes = zip.finish().unwrap().into_inner();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let thread = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stopped.load(Ordering::SeqCst) {
                    break;
                }
                let mut stream = stream.unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 2048];
                while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                    let count = stream.read(&mut buffer).unwrap();
                    if count == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..count]);
                    assert!(
                        request.len() <= 16 * 1024,
                        "fixture request header exceeds limit"
                    );
                }
                let request = String::from_utf8(request).unwrap();
                let path = request.split_whitespace().nth(1).unwrap();
                let body = if path == "/repos/fixture/skills/commits/main" {
                    commit.as_bytes()
                } else if path == format!("/fixture/skills/zip/{commit}") {
                    &bytes
                } else {
                    panic!("unexpected skill fixture path: {path}");
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(body).unwrap();
            }
        });
        Self {
            endpoint: format!("http://{address}"),
            address,
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = std::net::TcpStream::connect(self.address);
        self.thread.take().unwrap().join().unwrap();
    }
}
