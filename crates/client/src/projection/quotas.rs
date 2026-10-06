use sailry_protocol::{ProviderId, Snapshot, conversation::quota};

fn selected(snapshot: &Snapshot, quota: &quota::Snapshot) -> bool {
    quota.credential.node == snapshot.node
        && snapshot.providers.iter().any(|provider| {
            provider.id == quota.provider && provider.credential.as_ref() == Some(&quota.credential)
        })
}

pub(super) fn retain(snapshot: &mut Snapshot) {
    let quotas = std::mem::take(&mut snapshot.provider_quotas);
    snapshot.provider_quotas = quotas
        .into_iter()
        .filter(|quota| selected(snapshot, quota))
        .collect();
}

pub(super) fn apply(snapshot: &mut Snapshot, provider: ProviderId, quota: Option<quota::Snapshot>) {
    snapshot
        .provider_quotas
        .retain(|quota| quota.provider != provider);
    if let Some(quota) = quota
        && quota.provider == provider
        && selected(snapshot, &quota)
    {
        snapshot.provider_quotas.push(quota);
        snapshot.provider_quotas.sort_by_key(|quota| quota.provider);
    }
}
