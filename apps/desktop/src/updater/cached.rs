//! A staged install may reread only its pinned file; it cannot contact an update server.
use self_update::http_client::{HeaderMap, HttpClient, HttpResponse};
use std::{fs::File, path::PathBuf, time::Duration};

pub(super) struct Client {
    pub url: String,
    pub path: PathBuf,
}

struct Response {
    headers: HeaderMap,
    file: File,
}

impl HttpClient for Client {
    fn get(
        &self,
        url: &str,
        _: &HeaderMap,
        _: Option<Duration>,
    ) -> self_update::Result<Box<dyn HttpResponse>> {
        if url != self.url {
            return Err(self_update::Error::invalid_response(std::io::Error::other(
                "staged installation cannot request another update asset",
            )));
        }
        let file = File::open(&self.path)?;
        let mut headers = HeaderMap::new();
        headers.insert(
            self_update::http_client::header::CONTENT_LENGTH,
            file.metadata()?.len().to_string().parse().map_err(|_| {
                self_update::Error::invalid_response(std::io::Error::other(
                    "invalid staged asset length",
                ))
            })?,
        );
        Ok(Box::new(Response { headers, file }))
    }
}

impl HttpResponse for Response {
    fn headers(&self) -> &HeaderMap {
        &self.headers
    }
    fn body(self: Box<Self>) -> Box<dyn std::io::Read> {
        Box::new(self.file)
    }
}

pub(super) struct Source(pub self_update::Release);
impl self_update::ReleaseSource for Source {
    fn get_releases(&self) -> self_update::Result<Vec<self_update::Release>> {
        Ok(vec![self.0.clone()])
    }
}
