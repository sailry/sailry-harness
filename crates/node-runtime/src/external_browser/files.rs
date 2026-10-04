//! Browser files cross the worktree boundary through the existing confined file service.
use super::execute::*;
use base64::Engine;
use sailry_protocol::{Fault, RequestId, external_browser::Action};
use serde_json::{Value, json};
type Result<T> = std::result::Result<T, Fault>;

impl Call {
    pub(super) async fn upload(
        &self,
        directory: &std::path::Path,
        args: &mut Value,
    ) -> Result<tempfile::TempDir> {
        let relative = args
            .get("file_path")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("file_path is required"))?;
        let parts = crate::files::path::components(relative, false)?;
        let name = parts.last().expect("validated file path").to_string();
        let root = self.ingress.worktree_root(self.worktree).await?;
        let bytes = self
            .ingress
            .files
            .media_input(root, relative.into(), self.stop.clone())
            .await?;
        let directory = directory.to_path_buf();
        let (staging, path) = tokio::task::spawn_blocking(move || {
            let staging =
                tempfile::tempdir_in(directory).map_err(|error| invalid(error.to_string()))?;
            let path = staging.path().join(name);
            std::fs::write(&path, bytes).map_err(|error| invalid(error.to_string()))?;
            Ok::<_, Fault>((staging, path))
        })
        .await
        .map_err(|_| invalid("browser upload staging failed"))??;
        args["file_path"] = path
            .to_str()
            .ok_or_else(|| invalid("browser upload requires a UTF-8 path"))?
            .into();
        Ok(staging)
    }

    pub(super) async fn artifact(&self, id: RequestId, value: &mut Value) -> Result<()> {
        let (field, extension) = match self.action {
            Action::Screenshot => ("base64_image", "png"),
            Action::PrintToPdf => ("pdf_base64", "pdf"),
            _ => return Ok(()),
        };
        let encoded = value
            .as_object_mut()
            .and_then(|value| value.remove(field))
            .ok_or_else(|| invalid("browser artifact is missing its data"))?;
        let encoded = encoded
            .as_str()
            .ok_or_else(|| invalid("browser artifact data is invalid"))?;
        if encoded.len() > 28 * 1024 * 1024 {
            return Err(invalid("browser artifact exceeds 20 MiB"));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| invalid("invalid browser artifact encoding"))?;
        if bytes.len() > 20 * 1024 * 1024 {
            return Err(invalid("browser artifact exceeds 20 MiB"));
        }
        value["path"] = self
            .save(format!("browser-{id}.{extension}"), bytes)
            .await?
            .into();
        Ok(())
    }

    pub(super) async fn download(
        &self,
        session: &crate::external_browser::Session,
        id: RequestId,
        args: &Value,
    ) -> Result<Value> {
        let directory = session.directory.join("downloads");
        if self.action == Action::Downloads {
            let files = tokio::task::spawn_blocking(move || {
                let root = crate::files::path::root(&directory)?;
                let mut files = Vec::new();
                for entry in root.entries().map_err(|error| invalid(error.to_string()))? {
                    let entry = entry.map_err(|error| invalid(error.to_string()))?;
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name.ends_with(".crdownload") || !entry.file_type().is_ok_and(|kind| kind.is_file()) { continue; }
                    files.push(json!({"name":name,"size":entry.metadata().map_err(|error| invalid(error.to_string()))?.len()}));
                    if files.len() >= 128 { break; }
                }
                Ok::<_, Fault>(files)
            }).await.map_err(|_| invalid("browser download listing failed"))??;
            return Ok(json!({"execution_node":self.ingress.node,"files":files,"limit":128}));
        }
        let name = args
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("download name is required"))?;
        if crate::files::path::components(name, false)?.len() != 1 || name.ends_with(".crdownload")
        {
            return Err(invalid("select a completed download file name"));
        }
        let bytes = self
            .ingress
            .files
            .media_input(directory, name.into(), self.stop.clone())
            .await?;
        let path = self.save(format!("browser-{id}-{name}"), bytes).await?;
        Ok(json!({"execution_node":self.ingress.node,"path":path}))
    }

    async fn save(&self, name: String, bytes: Vec<u8>) -> Result<String> {
        let root = self.ingress.worktree_root(self.worktree).await?;
        let output = self
            .ingress
            .files
            .prepare_media(root, format!("assets/generated/{name}"), 20 * 1024 * 1024)
            .await?;
        self.ingress
            .files
            .publish_media(output, bytes, self.stop.clone())
            .await
    }
}
