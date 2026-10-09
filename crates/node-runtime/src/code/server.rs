use super::{
    documents::{self, Documents},
    positions::Encoding,
    resolve::Plan,
    rpc::Rpc,
};
use lsp_types::{InitializeResult, ServerCapabilities};
use sailry_link::CancellationToken;
use sailry_protocol::{
    Fault,
    code::{Overlay, Status},
};
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

pub(super) struct Server {
    pub plan: Plan,
    pub rpc: Rpc,
    pub capabilities: ServerCapabilities,
    pub encoding: Encoding,
    pub documents: Mutex<Documents>,
    pub used: Mutex<Instant>,
}

impl Server {
    pub async fn start(
        plan: Plan,
        environment: &crate::process::Environment,
        closed: CancellationToken,
        stop: &CancellationToken,
    ) -> Result<Arc<Self>, Fault> {
        let rpc = Rpc::spawn(&plan, environment, closed)?;
        let root = url::Url::from_directory_path(&plan.directory)
            .map_err(|_| Fault::invalid("language root URI is unavailable"))?
            .to_string();
        let response = rpc.request("initialize", json!({
            "processId":std::process::id(),"clientInfo":{"name":"Sailry","version":"1"},
            "rootUri":root,"workspaceFolders":[{"uri":root,"name":plan.startup.root}],
            "initializationOptions":plan.config,"capabilities":{
                "general":{"positionEncodings":["utf-8","utf-16"]},
                "workspace":{"configuration":true,"workspaceFolders":true,"symbol":{"dynamicRegistration":false}},
                "window":{"workDoneProgress":true},
                "textDocument":{
                    "synchronization":{"dynamicRegistration":false,"didSave":true},
                    "publishDiagnostics":{"versionSupport":true},"diagnostic":{"dynamicRegistration":false},
                    "definition":{"linkSupport":true},"references":{"dynamicRegistration":false},
                    "hover":{"contentFormat":["plaintext","markdown"]},
                    "documentSymbol":{"hierarchicalDocumentSymbolSupport":true},
                    "completion":{"completionItem":{"snippetSupport":false}}
                }
            }
        }), plan.timeout, stop).await;
        let result = match response.and_then(|value| {
            serde_json::from_value::<InitializeResult>(value).map_err(|error| {
                Fault::invalid(format!("invalid language initialization: {error}"))
            })
        }) {
            Ok(result) => result,
            Err(error) => {
                rpc.shutdown().await;
                return Err(error);
            }
        };
        let encoding = match Encoding::from(result.capabilities.position_encoding.as_ref()) {
            Ok(encoding) => encoding,
            Err(error) => {
                rpc.shutdown().await;
                return Err(error);
            }
        };
        rpc.notify("initialized", json!({})).await?;
        rpc.notify(
            "workspace/didChangeConfiguration",
            json!({"settings":plan.config}),
        )
        .await?;
        {
            let mut state = rpc.state.lock().await;
            if state.status == Status::Starting {
                state.status = Status::Ready;
            }
        }
        Ok(Arc::new(Self {
            plan,
            rpc,
            capabilities: result.capabilities,
            encoding,
            documents: Mutex::new(Documents::default()),
            used: Mutex::new(Instant::now()),
        }))
    }

    pub async fn synchronize(
        &self,
        path: &str,
        overlay: Option<&Overlay>,
        save: bool,
    ) -> Result<(), Fault> {
        let mut text = documents::read(&self.plan.root, path)?;
        let saved_revision = text.revision.clone();
        if let Some(overlay) = overlay {
            if overlay.path != path || overlay.expected_revision != text.revision {
                return Err(Fault::new(
                    sailry_protocol::ErrorCode::RevisionConflict,
                    "code draft differs from the saved document",
                ));
            }
            if overlay.text.len() > documents::MAX_TEXT || overlay.text.contains('\0') {
                return Err(Fault::invalid(
                    "code draft exceeds the text limit or contains binary data",
                ));
            }
            text.text = overlay.text.clone();
            text.revision = blake3::hash(text.text.as_bytes()).to_hex().to_string();
        }
        let mut documents = self.documents.lock().await;
        let (open, changed, version, evicted) = documents.put(path.into(), text, saved_revision);
        if let Some(evicted) = evicted {
            self.rpc
                .notify(
                    "textDocument/didClose",
                    json!({"textDocument":{"uri":documents::uri(&self.plan.root, &evicted)?}}),
                )
                .await?;
        }
        let uri = documents::uri(&self.plan.root, path)?;
        let document = &documents.0[path];
        if open {
            self.rpc.notify("textDocument/didOpen", json!({"textDocument":{"uri":uri,"languageId":self.plan.rules.language(path,&document.text.text).map(|language|language.id.as_str()).unwrap_or(&self.plan.language),"version":version,"text":document.text.text}})).await?;
        } else if changed {
            self.rpc.notify("textDocument/didChange", json!({"textDocument":{"uri":uri,"version":version},"contentChanges":[{"text":document.text.text}]})).await?;
        }
        if save && overlay.is_none() {
            self.rpc
                .notify(
                    "textDocument/didSave",
                    json!({"textDocument":{"uri":uri},"text":document.text.text}),
                )
                .await?;
        }
        *self.used.lock().await = Instant::now();
        Ok(())
    }

    pub async fn refresh(&self) -> Result<(), Fault> {
        let paths = self
            .documents
            .lock()
            .await
            .0
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for path in paths {
            match documents::read(&self.plan.root, &path) {
                Ok(saved) => {
                    let changed = self
                        .documents
                        .lock()
                        .await
                        .0
                        .get(&path)
                        .is_some_and(|document| document.saved_revision != saved.revision);
                    if changed {
                        self.synchronize(&path, None, false).await?;
                    }
                }
                Err(error) if error.code == sailry_protocol::ErrorCode::NotFound => {
                    self.close(&path).await?
                }
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    pub async fn close(&self, path: &str) -> Result<(), Fault> {
        if self.documents.lock().await.0.remove(path).is_some() {
            self.rpc
                .notify(
                    "textDocument/didClose",
                    json!({"textDocument":{"uri":documents::uri(&self.plan.root, path)?}}),
                )
                .await?;
        }
        self.rpc
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes":[{"uri":documents::uri(&self.plan.root, path)?,"type":3}]}),
            )
            .await
    }

    pub async fn settle(&self, stop: &CancellationToken) {
        let end = tokio::time::Instant::now() + Duration::from_secs(3);
        let mut changed = self.rpc.changed.clone();
        loop {
            tokio::select! {
                biased;
                _ = stop.cancelled() => return,
                _ = tokio::time::sleep_until(end) => return,
                result = tokio::time::timeout(Duration::from_millis(300), changed.changed()) => if !matches!(result, Ok(Ok(()))) { return; },
            }
        }
    }

    pub async fn request(
        &self,
        method: &str,
        params: Value,
        stop: &CancellationToken,
    ) -> Result<Value, Fault> {
        self.rpc
            .request(method, params, self.plan.timeout, stop)
            .await
    }
}
