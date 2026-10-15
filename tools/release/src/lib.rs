//! Release-only signing and authenticated update-feed production.

mod archive;
mod feed;
mod key;

use std::{
    error::Error,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
};

pub use feed::Channel;

pub type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

pub struct Describe<'a> {
    pub archive: &'a Path,
    pub keys: &'a Path,
    pub minimum_system: &'a str,
    pub output: &'a Path,
    pub repository: &'a str,
    pub tag: &'a str,
    pub target: &'a str,
    pub version: &'a str,
}

pub struct Merge<'a> {
    pub assets: &'a Path,
    pub channel: Channel,
    pub feed: &'a Path,
    pub keys: &'a Path,
    pub output: &'a Path,
    pub repository: &'a str,
    pub tag: &'a str,
}

pub fn sign_archive(input: &Path, output: &Path, name: &str, keys: &Path) -> Result<()> {
    let ring = key::read_ring(keys)?;
    let signer = key::read_signer(&ring)?;
    archive::sign(input, output, name, &signer, &ring)
}

pub fn describe(options: Describe<'_>) -> Result<()> {
    let ring = key::read_ring(options.keys)?;
    let release = feed::describe(
        options.archive,
        options.version,
        options.target,
        options.minimum_system,
        options.repository,
        options.tag,
        &ring,
    )?;
    let mut bytes = serde_json::to_vec(&release)?;
    bytes.push(b'\n');
    write_new(options.output, &bytes)
}

pub fn merge(options: Merge<'_>) -> Result<()> {
    let ring = key::read_ring(options.keys)?;
    let signer = key::read_signer(&ring)?;
    let current = fs::read(options.feed)?;
    let next = feed::merge(
        &current,
        options.assets,
        options.channel,
        options.repository,
        options.tag,
        &ring,
        &signer,
    )?;
    write_new(options.output, &next)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[cfg(test)]
mod tests;
