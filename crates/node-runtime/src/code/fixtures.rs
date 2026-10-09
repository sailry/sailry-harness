//! Isolated test-support assets; production builds do not expose fixture controls.
use std::{io::Write, path::Path};

pub struct CodeServerFixture {
    pub asset_url: String,
    pub release_url: String,
    pub version: String,
}
impl Default for CodeServerFixture {
    fn default() -> Self {
        Self::new()
    }
}
impl CodeServerFixture {
    pub fn new() -> Self {
        super::install::fixture()
    }
    pub fn script() -> &'static str {
        include_str!("tests/server.py")
    }
    pub fn archive(python: impl AsRef<Path>) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        write!(
            &mut encoder,
            "#!{}\n{}",
            python.as_ref().display(),
            Self::script()
        )
        .expect("fixture archive is writable");
        encoder.finish().expect("fixture archive is complete")
    }
    pub fn asset_path(&self) -> String {
        let url = url::Url::parse(&self.asset_url).expect("fixture asset URL");
        format!("/{}{}", url.host_str().unwrap(), url.path())
    }
    pub fn release_path(&self) -> String {
        let url = url::Url::parse(&self.release_url).expect("fixture release URL");
        format!("/{}{}", url.host_str().unwrap(), url.path())
    }
}
