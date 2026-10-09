use super::*;
use sailry_protocol::{ErrorCode, models::Format};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

mod compatibility;
mod discovery;
mod mirrors;
mod prefixes;
mod resolution;
mod server;
mod versions;

use server::{Reply, Request, Server};

const REVISION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OLDER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const NEWER: &str = "cccccccccccccccccccccccccccccccccccccccc";

struct Catalog {
    files: BTreeMap<String, Data>,
}

struct Data {
    body: Vec<u8>,
    lfs: bool,
    revision: &'static str,
}

impl Catalog {
    fn embedding() -> Self {
        let mut catalog = Self {
            files: BTreeMap::new(),
        };
        catalog.insert(
            "config.json",
            &json!({"hidden_size":384,"type_vocab_size":2,"model_type":"bert"}),
        );
        catalog.insert(
            "tokenizer.json",
            &json!({"version":"1.0","model":{"type":"WordPiece"}}),
        );
        catalog.insert(
            "1_Pooling/config.json",
            &json!({
                "word_embedding_dimension":384,
                "pooling_mode_cls_token":true,
                "pooling_mode_mean_tokens":false,
                "pooling_mode_max_tokens":false
            }),
        );
        catalog.files.insert(
            "onnx/model.onnx".into(),
            Data {
                body: b"resolver graph metadata fixture".to_vec(),
                lfs: true,
                revision: NEWER,
            },
        );
        catalog
    }

    fn speech() -> Self {
        let mut catalog = Self {
            files: BTreeMap::new(),
        };
        catalog.files.insert(
            "tokens.txt".into(),
            Data {
                body: "<unk> 0\n你 1\nhello 2\n".as_bytes().to_vec(),
                lfs: false,
                revision: OLDER,
            },
        );
        for path in ["model.onnx", "model.int8.onnx"] {
            catalog.files.insert(
                path.into(),
                Data {
                    body: format!("resolver SenseVoice graph metadata fixture {path}").into_bytes(),
                    lfs: true,
                    revision: NEWER,
                },
            );
        }
        catalog
    }

    fn insert(&mut self, path: &str, value: &Value) {
        self.files.insert(
            path.into(),
            Data {
                body: serde_json::to_vec(value).unwrap(),
                lfs: false,
                revision: OLDER,
            },
        );
    }

    fn hf_info(&self, revision: &str) -> Value {
        let siblings = self
            .files
            .iter()
            .map(|(path, data)| {
                let mut file = json!({
                    "rfilename":path,
                    "blobId":blob(&data.body),
                    "size":data.body.len()
                });
                if data.lfs {
                    file["lfs"] = json!({"sha256":checksum(&data.body),"size":data.body.len()});
                }
                file
            })
            .collect::<Vec<_>>();
        json!({"id":"org/model","sha":revision,"gated":false,"private":false,"cardData":{"license":"mit"},"siblings":siblings})
    }

    fn ms_tree(&self) -> Value {
        let files = self
            .files
            .iter()
            .map(|(path, data)| {
                json!({
                    "Path":path,"Size":data.body.len(),"Sha256":checksum(&data.body),
                    "Revision":data.revision,"Type":"blob"
                })
            })
            .collect::<Vec<_>>();
        json!({"Success":true,"Data":{"Files":files}})
    }

    fn hf_reply(&self, request: &Request) -> Reply {
        if request.path.starts_with("/api/models/") && request.path.contains("/revision/") {
            return Reply::json(self.hf_info(REVISION));
        }
        let marker = format!("/resolve/{REVISION}/");
        if let Some((_, path)) = request.path.split_once(&marker)
            && let Some(data) = self.files.get(path)
        {
            return Reply::body(data.body.clone());
        }
        Reply::status(404)
    }

    fn ms_reply(&self, request: &Request) -> Reply {
        if request.path.starts_with("/openapi/v1/models/") {
            return Reply::json(
                json!({"success":true,"data":{"display_name":"Fixture model","license":"mit","private":false,"gated":false}}),
            );
        }
        if request.path.ends_with("/info/refs") {
            return Reply::status(421);
        }
        if request.path.ends_with("/revisions") {
            return Reply::json(
                json!({"Success":true,"Data":{"RevisionMap":{"Branches":[{"Revision":"master"}],"Tags":[{"Revision":"v1"}]}}}),
            );
        }
        if request.path.ends_with("/repo/files") {
            return Reply::json(self.ms_tree());
        }
        if request.path.ends_with("/repo")
            && let Some(path) = request.query.get("FilePath")
            && let Some(data) = self.files.get(path)
        {
            return Reply::body(data.body.clone());
        }
        Reply::status(404)
    }
}

fn source(hub: Hub, revision: &str) -> Source {
    Source {
        hub,
        repository: "org/model".into(),
        revision: revision.into(),
    }
}

fn checksum(body: &[u8]) -> String {
    format!("{:x}", Sha256::digest(body))
}

fn blob(body: &[u8]) -> String {
    let mut hash = sha1::Sha1::new();
    hash.update(format!("blob {}\0", body.len()));
    hash.update(body);
    format!("{:x}", hash.finalize())
}

fn advertisement(refs: &[(&str, &str)]) -> Vec<u8> {
    let mut body = Vec::new();
    let service = "# service=git-upload-pack\n";
    body.extend_from_slice(format!("{:04x}{service}0000", service.len() + 4).as_bytes());
    for (index, (name, revision)) in refs.iter().enumerate() {
        let line = format!(
            "{revision} {name}{}\n",
            if index == 0 { "\0multi_ack" } else { "" }
        );
        body.extend_from_slice(format!("{:04x}{line}", line.len() + 4).as_bytes());
    }
    body.extend_from_slice(b"0000");
    body
}
