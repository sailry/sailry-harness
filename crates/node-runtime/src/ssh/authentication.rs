use super::*;
use russh::keys::{
    PrivateKeyWithHashAlg,
    agent::{AgentIdentity, client::AgentClient},
    decode_secret_key,
};

pub(super) async fn authenticate(
    session: &mut client::Handle<Verifier>,
    username: &str,
    credential: &Credential,
) -> Result<bool, Fault> {
    match credential {
        Credential::Password { password } => session
            .authenticate_password(username, password.expose())
            .await
            .map(|result| result.success())
            .map_err(|_| unavailable("SSH password authentication failed")),
        Credential::PrivateKey { key, passphrase } => {
            key_auth(session, username, key, passphrase.as_ref()).await
        }
        Credential::KeyPath { path, passphrase } => {
            use tokio::io::AsyncReadExt;
            let file = tokio::fs::File::open(path.expose()).await.map_err(|_| {
                unavailable("SSH private key could not be opened on the execution Node")
            })?;
            let mut contents = zeroize::Zeroizing::new(String::new());
            file.take(65537)
                .read_to_string(&mut contents)
                .await
                .map_err(|_| unavailable("SSH private key could not be read"))?;
            if contents.len() > 65536 {
                return Err(Fault::new(
                    ErrorCode::InvalidRequest,
                    "SSH private key exceeds the size limit",
                ));
            }
            key_auth(
                session,
                username,
                &Secret::new(contents.to_string()),
                passphrase.as_ref(),
            )
            .await
        }
        Credential::Agent => agent_auth(session, username).await,
    }
}

async fn key_auth(
    session: &mut client::Handle<Verifier>,
    username: &str,
    contents: &Secret,
    passphrase: Option<&Secret>,
) -> Result<bool, Fault> {
    let key =
        decode_secret_key(contents.expose(), passphrase.map(Secret::expose)).map_err(|_| {
            Fault::new(
                ErrorCode::InvalidRequest,
                "SSH private key or passphrase is invalid",
            )
        })?;
    let hash = session
        .best_supported_rsa_hash()
        .await
        .map_err(|_| unavailable("SSH key negotiation failed"))?
        .flatten();
    session
        .authenticate_publickey(username, PrivateKeyWithHashAlg::new(Arc::new(key), hash))
        .await
        .map(|result| result.success())
        .map_err(|_| unavailable("SSH key authentication failed"))
}

async fn agent_auth(session: &mut client::Handle<Verifier>, username: &str) -> Result<bool, Fault> {
    #[cfg(unix)]
    let mut agent = AgentClient::connect_env()
        .await
        .map_err(|_| unavailable("SSH agent is unavailable on the execution Node"))?;
    #[cfg(windows)]
    let mut agent = AgentClient::connect_named_pipe(r"\\.\pipe\openssh-ssh-agent")
        .await
        .map_err(|_| unavailable("SSH agent is unavailable on the execution Node"))?;
    let keys = agent
        .request_identities()
        .await
        .map_err(|_| unavailable("SSH agent identities are unavailable"))?;
    let hash = session
        .best_supported_rsa_hash()
        .await
        .map_err(|_| unavailable("SSH key negotiation failed"))?
        .flatten();
    for identity in keys {
        let AgentIdentity::PublicKey { key, .. } = identity else {
            continue;
        };
        if session
            .authenticate_publickey_with(username, key, hash, &mut agent)
            .await
            .map_err(|_| unavailable("SSH agent authentication failed"))?
            .success()
        {
            return Ok(true);
        }
    }
    Ok(false)
}
