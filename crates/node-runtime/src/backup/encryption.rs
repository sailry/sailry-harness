//! Password-protected age streams are authenticated before profile restoration.
use super::{MAX_BYTES, archive, cancelled, io};
use age::secrecy::SecretString;
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, Secret};
use std::io::{Read, Write};

#[cfg(test)]
mod tests;

// Bounds memory use to approximately 256 MiB, including on small execution Nodes.
const WORK_FACTOR: u8 = 18;

pub(crate) fn passphrase(value: Option<&Secret>) -> Result<&Secret, Fault> {
    value
        .filter(|value| {
            !value.expose().is_empty()
                && value.expose().len() <= 8192
                && !value.expose().chars().any(char::is_control)
        })
        .ok_or_else(|| Fault::new(ErrorCode::NotConfigured, "backup passphrase is required"))
}

pub(crate) fn seal(
    mut packed: archive::Packed,
    passphrase: &Secret,
    cancel: &CancellationToken,
) -> Result<archive::Packed, Fault> {
    cancelled(cancel)?;
    let mut recipient =
        age::scrypt::Recipient::new(SecretString::from(passphrase.expose().to_owned()));
    recipient.set_work_factor(WORK_FACTOR);
    let encryptor =
        age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
            .map_err(|_| Fault::invalid("backup encryption could not be initialized"))?;
    let file = tempfile::NamedTempFile::new_in(packed.snapshot.directory.path()).map_err(io)?;
    let mut output = encryptor
        .wrap_output(file.reopen().map_err(io)?)
        .map_err(io)?;
    transfer(&mut packed.file.reopen().map_err(io)?, &mut output, cancel)?;
    let output = output.finish().map_err(io)?;
    output.sync_all().map_err(io)?;
    let size = output.metadata().map_err(io)?.len();
    if size > MAX_BYTES {
        return Err(Fault::invalid("encrypted backup exceeds the export limit"));
    }
    packed.sha256 = archive::hash(&mut file.reopen().map_err(io)?, size, cancel)?;
    packed.size = size;
    packed.file = file;
    Ok(packed)
}

pub(crate) fn open(
    input: &tempfile::NamedTempFile,
    passphrase: &Secret,
    cancel: &CancellationToken,
) -> Result<tempfile::NamedTempFile, Fault> {
    cancelled(cancel)?;
    let decryptor = age::Decryptor::new(input.reopen().map_err(io)?).map_err(decryption)?;
    if !decryptor.is_scrypt() {
        return Err(decryption(()));
    }
    let mut identity =
        age::scrypt::Identity::new(SecretString::from(passphrase.expose().to_owned()));
    identity.set_max_work_factor(WORK_FACTOR);
    let mut reader = decryptor
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .map_err(decryption)?;
    let file = tempfile::NamedTempFile::new_in(input.path().parent().unwrap()).map_err(io)?;
    let mut output = file.reopen().map_err(io)?;
    transfer(&mut reader, &mut output, cancel).map_err(|error| {
        if error.code == ErrorCode::Cancelled {
            error
        } else {
            decryption(())
        }
    })?;
    output.sync_all().map_err(io)?;
    Ok(file)
}

fn transfer(
    input: &mut impl Read,
    output: &mut impl Write,
    cancel: &CancellationToken,
) -> Result<(), Fault> {
    let mut buffer = [0; 64 * 1024];
    let mut total = 0u64;
    loop {
        cancelled(cancel)?;
        let size = input.read(&mut buffer).map_err(io)?;
        if size == 0 {
            return Ok(());
        }
        total += size as u64;
        if total > MAX_BYTES {
            return Err(Fault::invalid("backup exceeds the transfer limit"));
        }
        output.write_all(&buffer[..size]).map_err(io)?;
    }
}

fn decryption(_: impl std::fmt::Debug) -> Fault {
    Fault::invalid("backup could not be decrypted; check the passphrase and archive")
}
