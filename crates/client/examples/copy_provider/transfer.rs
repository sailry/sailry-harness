use sailry_client::Client;
use sailry_protocol::{
    Authentication, Command, Output, ProviderId, Request, RequestOutcome,
    conversation::{ModelApi, Provider, presets},
};
use std::{fmt::Display, time::Duration};

pub(super) const NAME: &str = "星桥";
pub(super) type Result<T> = std::result::Result<T, &'static str>;
const WAIT: Duration = Duration::from_secs(45);

pub(super) struct Plan {
    source: Provider,
    draft: Provider,
    before: Vec<Provider>,
}

pub(super) fn redacted(id: impl Display) -> String {
    let id = id.to_string();
    format!("{}..{}", &id[..8], &id[id.len() - 4..])
}

pub(super) async fn prepare(
    source: &Client,
    target: &Client,
    revision: Option<u64>,
) -> Result<Plan> {
    let provider = source_metadata(source, target).await?;
    if revision.is_some_and(|revision| revision != provider.revision) {
        return Err("source_provider_revision_changed");
    }
    compatible(source, target, &provider).await?;
    let before = providers(target).await?;
    for existing in &before {
        if existing.name == NAME
            || (existing.api == provider.api
                && existing.authentication == provider.authentication
                && existing.endpoint == provider.endpoint
                && existing.preset == provider.preset)
        {
            println!(
                "status=target_matching_provider_exists provider={} revision={}",
                redacted(existing.id),
                existing.revision
            );
            return Err("target_matching_provider_exists_no_copy");
        }
    }
    let mut draft = provider.clone();
    draft.id = ProviderId::new();
    draft.revision = 0;
    draft.credential = None;
    if draft.id == provider.id || before.iter().any(|item| item.id == draft.id) {
        return Err("new_provider_id_collision_no_copy");
    }
    Ok(Plan {
        source: provider,
        draft,
        before,
    })
}

async fn source_metadata(source: &Client, target: &Client) -> Result<Provider> {
    if source.target() == target.target() {
        return Err("source_and_target_must_differ");
    }
    let provider = select(&providers(source).await?)?;
    println!(
        "status=source_exact_match provider={} revision={}",
        redacted(provider.id),
        provider.revision
    );
    if provider.authentication != Authentication::ApiKey {
        return Err("only_api_key_authentication_is_supported");
    }
    if provider.revision == 0
        || !provider
            .credential
            .as_ref()
            .is_some_and(|reference| reference.node == source.target())
    {
        return Err("source_requires_node_owned_api_key");
    }
    Ok(provider)
}

fn select(items: &[Provider]) -> Result<Provider> {
    let mut matches = items.iter().filter(|provider| provider.name == NAME);
    let provider = matches.next().ok_or("exact_provider_name_not_found")?;
    if matches.next().is_some() {
        return Err("exact_provider_name_is_ambiguous");
    }
    Ok(provider.clone())
}

async fn compatible(source: &Client, target: &Client, provider: &Provider) -> Result<()> {
    if provider.api == ModelApi::Plugin
        || provider.consent.is_some()
        || provider.binding.as_ref().is_some_and(|binding| {
            binding.package.is_some()
                || binding.origin.is_some()
                || binding.auth.is_some()
                || binding.adapter.is_some()
        })
        || matches!(
            provider.preset.as_ref().map(|preset| &preset.source),
            Some(presets::Source::Package { .. })
        )
    {
        return Err("package_binding_requires_target_owned_configuration_no_copy");
    }
    let Some(reference) = &provider.preset else {
        return if provider.binding.is_none() {
            Ok(())
        } else {
            Err("captured_binding_is_not_portable_no_copy")
        };
    };
    let left = entry(source, reference).await?;
    let right = entry(target, reference).await?;
    if left.preset != right.preset
        || left.package.is_some()
        || right.package.is_some()
        || left.origin.is_some()
        || right.origin.is_some()
    {
        return Err("channel_declarations_differ_no_copy");
    }
    if !matches!(
        left.preset.auth,
        presets::Auth::ApiKey { .. } | presets::Auth::None
    ) {
        return Err("channel_authentication_is_not_portable_no_copy");
    }
    let routes = left
        .preset
        .routes
        .iter()
        .filter(|route| {
            route.protocol.api() == provider.api
                && reference
                    .route
                    .as_ref()
                    .is_none_or(|key| route.key() == *key)
        })
        .count();
    if routes != 1 {
        return Err("channel_route_missing_or_ambiguous_no_copy");
    }
    if matches!(reference.source, presets::Source::Partner) {
        if provider.api == ModelApi::ChatCompletions {
            return Err("partner_chat_policy_cannot_be_proven_portable_no_copy");
        }
        let key_injection = match left.preset.auth {
            presets::Auth::ApiKey { header, prefix } if header.is_some() || prefix.is_some() => {
                Some(sailry_protocol::plugin::model_auth::Injection {
                    header: header.unwrap_or_else(|| "authorization".into()),
                    template: format!("{}{{key}}", prefix.as_deref().unwrap_or("")),
                })
            }
            _ => None,
        };
        let binding = presets::Binding {
            policies: left.preset.policies,
            key_injection,
            ..Default::default()
        };
        if provider.binding.as_deref() != Some(&binding) {
            return Err("saved_partner_policy_differs_from_directory_no_copy");
        }
    } else if provider.binding.is_some() {
        return Err("captured_binding_is_not_portable_no_copy");
    }
    Ok(())
}

async fn entry(client: &Client, reference: &presets::PresetRef) -> Result<presets::Entry> {
    let output = read(
        client,
        Command::ListChannelPresets {
            refresh_partners: false,
        },
    )
    .await?;
    let Output::ChannelPresets(listing) = output else {
        return Err("channel_directory_response_expected");
    };
    let mut matches = listing
        .entries
        .into_iter()
        .filter(|entry| entry.source == reference.source && entry.preset.id == reference.id);
    let entry = matches
        .next()
        .ok_or("channel_unavailable_on_node_no_copy")?;
    if matches.next().is_some() {
        return Err("channel_directory_is_ambiguous_no_copy");
    }
    Ok(entry)
}

pub(super) async fn copy(source: &Client, target: &Client, plan: Plan) -> Result<()> {
    if !same_set(&plan.before, &providers(target).await?) {
        return Err("target_changed_before_key_read_no_copy");
    }
    let output = read(
        source,
        Command::ReadProviderKey {
            provider: plan.source.id,
            expected_revision: plan.source.revision,
        },
    )
    .await?;
    let Output::ProviderKey(Some(secret)) = output else {
        return Err("source_api_key_unavailable_no_copy");
    };
    let request = target.prepare(Command::SaveProvider {
        provider: plan.draft.clone(),
        expected_revision: 0,
        secret: Some(secret),
    });
    println!(
        "status=single_save_attempt request={} provider={}",
        redacted(request.id),
        redacted(plan.draft.id)
    );
    let saved = save(target, &request).await;
    drop(request);
    let saved = saved?;
    verify_saved(target, &plan.draft, &saved)?;
    let after = providers(target).await?;
    if after
        .iter()
        .filter(|provider| provider.name == NAME)
        .count()
        != 1
        || after.iter().find(|provider| provider.id == saved.id) != Some(&saved)
        || !same_set(
            &plan.before,
            &after
                .into_iter()
                .filter(|provider| provider.id != saved.id)
                .collect::<Vec<_>>(),
        )
    {
        return Err("target_changed_after_save_no_rollback_or_replay");
    }
    println!(
        "status=saved_metadata_verified provider={} revision={}",
        redacted(saved.id),
        saved.revision
    );
    validate(target, &saved).await
}

async fn save(target: &Client, request: &Request) -> Result<Provider> {
    if let Ok(Ok(Output::Provider(provider))) =
        tokio::time::timeout(WAIT, target.execute(request.clone())).await
    {
        return Ok(provider);
    }
    println!(
        "status=save_completion_unconfirmed_inspecting_original request={}",
        redacted(request.id)
    );
    let outcome = tokio::time::timeout(WAIT, target.outcome(request)).await;
    match outcome {
        Ok(Ok(RequestOutcome::Completed(result))) => match *result {
            Ok(Output::Provider(provider)) => Ok(provider),
            _ => Err("save_failed_or_invalid_completion_no_replay"),
        },
        other => {
            if let Ok(items) = providers(target).await
                && let Command::SaveProvider {
                    provider: draft, ..
                } = &request.command
            {
                for provider in items
                    .iter()
                    .filter(|provider| provider.id == draft.id || provider.name == NAME)
                {
                    println!(
                        "status=unconfirmed_target_metadata provider={} revision={}",
                        redacted(provider.id),
                        provider.revision
                    );
                }
            }
            match other {
                Ok(Ok(RequestOutcome::Admitted)) => Err("save_pending_no_replay"),
                Ok(Ok(RequestOutcome::NotAdmitted)) => Err("save_not_observed_no_replay"),
                _ => Err("save_outcome_unknown_no_replay"),
            }
        }
    }
}

fn verify_saved(target: &Client, draft: &Provider, saved: &Provider) -> Result<()> {
    if saved.id != draft.id
        || saved.revision != 1
        || !saved
            .credential
            .as_ref()
            .is_some_and(|reference| reference.node == target.target())
    {
        return Err("saved_identity_revision_or_credential_mismatch_no_replay");
    }
    let mut expected = draft.clone();
    expected.revision = saved.revision;
    expected.credential = saved.credential.clone();
    if &expected != saved {
        return Err("saved_metadata_changed_no_replay");
    }
    Ok(())
}

pub(super) async fn validate_existing(source: &Client, target: &Client) -> Result<()> {
    let provider = source_metadata(source, target).await?;
    compatible(source, target, &provider).await?;
    let saved = select(&providers(target).await?)?;
    if saved.id == provider.id {
        return Err("target_must_have_independent_provider_id");
    }
    if saved.revision == 0
        || !saved
            .credential
            .as_ref()
            .is_some_and(|reference| reference.node == target.target())
    {
        return Err("target_requires_node_owned_api_key");
    }
    let mut expected = provider;
    expected.id = saved.id;
    expected.revision = saved.revision;
    expected.credential = saved.credential.clone();
    if expected != saved {
        return Err("target_metadata_differs_from_source_no_validation");
    }
    validate(target, &saved).await
}

async fn validate(target: &Client, provider: &Provider) -> Result<()> {
    let output = read(
        target,
        Command::ValidateProvider {
            provider: provider.id,
            expected_revision: provider.revision,
        },
    )
    .await?;
    let Output::ProviderValidation(validation) = output else {
        return Err("provider_validation_response_expected");
    };
    if validation.provider != provider.id || validation.revision != provider.revision {
        return Err("provider_validation_revision_mismatch");
    }
    if !validation.missing.is_empty()
        || !validation.exceeded.is_empty()
        || !validation.unverified.is_empty()
    {
        return Err("provider_validation_incomplete_no_model_generation");
    }
    if select(&providers(target).await?)? != *provider {
        return Err("target_changed_during_validation");
    }
    println!(
        "status=provider_validation_passed_no_model_generation provider={} revision={}",
        redacted(provider.id),
        provider.revision
    );
    Ok(())
}

fn same_set(left: &[Provider], right: &[Provider]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .all(|item| right.iter().find(|candidate| candidate.id == item.id) == Some(item))
}

async fn providers(client: &Client) -> Result<Vec<Provider>> {
    match read(client, Command::ListProviders).await? {
        Output::Providers(items) => Ok(items),
        _ => Err("provider_list_response_expected"),
    }
}

async fn read(client: &Client, command: Command) -> Result<Output> {
    let status = match &command {
        Command::ListProviders => "provider_list_read_failed_redacted",
        Command::ReadProviderKey { .. } => "provider_key_read_failed_redacted",
        Command::ListChannelPresets { .. } => "channel_directory_read_failed_redacted",
        Command::ValidateProvider { .. } => "provider_validation_request_failed_redacted",
        _ => return Err("unsupported_helper_read"),
    };
    tokio::time::timeout(WAIT, client.execute(client.prepare(command)))
        .await
        .map_err(|_| status)?
        .map_err(|_| status)
}
