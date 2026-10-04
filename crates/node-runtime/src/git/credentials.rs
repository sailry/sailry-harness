//! Git transport credentials remain on the execution Node.
pub(crate) fn callbacks<'a>() -> git2::RemoteCallbacks<'a> {
    let mut callbacks = git2::RemoteCallbacks::new();
    let mut attempts = 0;
    callbacks.credentials(move |url, username, allowed| {
        attempts += 1;
        if attempts > 3 {
            return Err(git2::Error::from_str("Git authentication failed"));
        }
        if allowed.contains(git2::CredentialType::SSH_KEY) {
            git2::Cred::ssh_key_from_agent(username.unwrap_or("git"))
        } else if allowed.contains(git2::CredentialType::USER_PASS_PLAINTEXT) {
            git2::Cred::credential_helper(&git2::Config::open_default()?, url, username)
        } else if allowed.contains(git2::CredentialType::USERNAME) {
            git2::Cred::username(username.unwrap_or("git"))
        } else {
            git2::Cred::default()
        }
    });
    callbacks
}
