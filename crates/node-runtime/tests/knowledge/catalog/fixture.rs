use sailry_link::CancellationToken;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::{JoinHandle, JoinSet},
};

pub(super) const REVISION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(super) const NEWER: &str = "cccccccccccccccccccccccccccccccccccccccc";

#[derive(Clone)]
pub(super) struct Fetch {
    pub path: String,
    pub query: BTreeMap<String, String>,
}

pub(super) struct Catalog {
    pub endpoint: String,
    requests: Arc<Mutex<Vec<Fetch>>>,
    stop: CancellationToken,
    task: JoinHandle<()>,
}

impl Catalog {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let base: reqwest::Url = endpoint.parse().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let stop = CancellationToken::new();
        let stopping = stop.clone();
        let task = tokio::spawn(async move {
            let mut peers = JoinSet::new();
            loop {
                let (mut socket, _) = tokio::select! {
                    biased;
                    _=stopping.cancelled()=>break,
                    peer=listener.accept()=>peer.unwrap(),
                    result=peers.join_next(),if !peers.is_empty()=>{result.unwrap().unwrap();continue;},
                };
                let recorded = recorded.clone();
                let base = base.clone();
                let stopping = stopping.clone();
                peers.spawn(async move {
                    let mut header=Vec::new();
                    while !header.ends_with(b"\r\n\r\n") {
                        let mut byte=[0];
                        let count=tokio::select! {biased;_=stopping.cancelled()=>return,count=socket.read(&mut byte)=>count.unwrap()};
                        if count==0 {return;}
                        header.extend_from_slice(&byte);
                        assert!(header.len()<16384);
                    }
                    let text=std::str::from_utf8(&header).unwrap();
                    let line=text.lines().next().unwrap();
                    assert_eq!(line.split_whitespace().next(),Some("GET"));
                    let url=base.join(line.split_whitespace().nth(1).unwrap()).unwrap();
                    let fetch=Fetch {path:url.path().into(),query:url.query_pairs().map(|(key,value)|(key.into_owned(),value.into_owned())).collect()};
                    recorded.lock().unwrap().push(fetch.clone());
                    let (status,body,extra)=reply(&fetch,&base);
                    let header=format!("HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n",body.len());
                    socket.write_all(header.as_bytes()).await.unwrap();
                    socket.write_all(&body).await.unwrap();
                });
            }
        });
        Self {
            endpoint,
            requests,
            stop,
            task,
        }
    }

    pub fn requests(&self) -> Vec<Fetch> {
        self.requests.lock().unwrap().clone()
    }
    pub fn clear(&self) {
        self.requests.lock().unwrap().clear();
    }
}

impl Drop for Catalog {
    fn drop(&mut self) {
        self.stop.cancel();
        self.task.abort();
    }
}

fn reply(request: &Fetch, base: &reqwest::Url) -> (u16, Vec<u8>, String) {
    if request.path == "/api/models" || request.path == "/openapi/v1/models" {
        let second = request.query.contains_key("cursor")
            || request.query.get("page_number").map(String::as_str) == Some("2");
        let repository = if second { "org/newer" } else { "org/model" };
        if request.path == "/api/models" {
            let next = if second {
                String::new()
            } else {
                format!(
                    "Link: <{}>; rel=\"next\"\r\n",
                    base.join("api/models?cursor=second").unwrap()
                )
            };
            return (
                200,
                serde_json::to_vec(&json!([{"id":repository}])).unwrap(),
                next,
            );
        }
        return json_reply(
            json!({"success":true,"data":{"models":[{"id":repository}],"total_count":21,"page_number":if second {2} else {1},"page_size":20}}),
        );
    }
    let revision = if request.path.contains("/newer/")
        || request.path.ends_with("/newer")
        || request.path.contains("/newer.git/")
    {
        NEWER
    } else {
        REVISION
    };
    let files = files();
    if request.path.contains("/revision/") {
        let siblings=files.iter().map(|(path,body)| json!({"rfilename":path,"size":body.len(),"lfs":{"sha256":checksum(body),"size":body.len()}})).collect::<Vec<_>>();
        return json_reply(
            json!({"sha":revision,"cardData":{"license":"mit"},"siblings":siblings}),
        );
    }
    if request.path.starts_with("/openapi/v1/models/") {
        return json_reply(
            json!({"success":true,"data":{"license":"mit","default_branch":"master"}}),
        );
    }
    if request.path.ends_with("/info/refs") {
        let service = "# service=git-upload-pack\n";
        let line = format!("{revision} refs/heads/master\0multi_ack\n");
        return (
            200,
            format!(
                "{:04x}{service}0000{:04x}{line}0000",
                service.len() + 4,
                line.len() + 4
            )
            .into_bytes(),
            String::new(),
        );
    }
    if request.path.ends_with("/repo/files") {
        let rows=files.iter().map(|(path,body)| json!({"Path":path,"Type":"blob","Size":body.len(),"Sha256":checksum(body),"Revision":revision})).collect::<Vec<_>>();
        return json_reply(json!({"Success":true,"Data":{"Files":rows}}));
    }
    let path = if request.path.ends_with("/repo") {
        request.query.get("FilePath").map(String::as_str)
    } else {
        request
            .path
            .split_once(&format!("/resolve/{revision}/"))
            .map(|(_, path)| path)
    };
    if let Some(body) = path.and_then(|path| files.get(path)) {
        return (200, body.clone(), String::new());
    }
    (404, Vec::new(), String::new())
}

fn files() -> BTreeMap<String, Vec<u8>> {
    [
        ("config.json",serde_json::to_vec(&json!({"model_type":"bert","hidden_size":8,"type_vocab_size":2})).unwrap()),
        ("1_Pooling/config.json",serde_json::to_vec(&json!({"word_embedding_dimension":8,"pooling_mode_cls_token":true,"pooling_mode_mean_tokens":false})).unwrap()),
        ("tokenizer.json",b"immutable tokenizer metadata fixture".to_vec()),
        ("onnx/model.onnx",b"immutable graph metadata fixture".to_vec()),
        ("ocr.json",serde_json::to_vec(&json!({"family":"v5", "detector":"pp-ocrv5_mobile_det.onnx", "recognizer":"pp-ocrv5_mobile_rec.onnx", "dictionary":"ppocrv5_dict.txt"})).unwrap()),
        ("pp-ocrv5_mobile_det.onnx",b"mobile detector metadata".to_vec()),
        ("pp-ocrv5_mobile_rec.onnx",b"mobile recognizer metadata".to_vec()),
        ("ppocrv5_dict.txt", "你\n好\n".as_bytes().to_vec()),
        ("pp-ocrv6_tiny_det.onnx",b"tiny detector metadata".to_vec()),
        ("pp-ocrv6_tiny_rec.onnx",b"tiny recognizer metadata".to_vec()),
        ("ppocrv6_tiny_dict.txt", b"a\nb\n".to_vec()),
    ].into_iter().map(|(path,body)|(path.into(),body)).collect()
}
fn checksum(body: &[u8]) -> String {
    format!("{:x}", Sha256::digest(body))
}
fn json_reply(value: Value) -> (u16, Vec<u8>, String) {
    (200, serde_json::to_vec(&value).unwrap(), String::new())
}
