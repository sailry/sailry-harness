//! Declared provider resources retain the existing authorization and HTTP owners.
use super::*;
use crate::store::database::Database;
use sailry_protocol::{conversation::consent::Scope, plugin};

pub(in crate::store) async fn prepare(
    db: &Database,
    caller: NodeId,
    request: &Request,
) -> Result<crate::plugins::http::Input, Fault> {
    db.check_plugin(caller, request).await?;
    let context = request.plugin.as_ref().ok_or_else(denied)?;
    let (provider, expected_revision, name, input, mutation) = match &request.command {
        Command::RequestProviderHttp {
            provider,
            expected_revision,
            request,
            input,
        } => (provider, expected_revision, request, input, false),
        Command::ExecuteProviderHttp {
            provider,
            expected_revision,
            request,
            input,
        } => (provider, expected_revision, request, input, true),
        _ => return Err(Fault::invalid("provider resource command expected")),
    };
    let mut provider = super::current(&db.connection, *provider, *expected_revision).await?;
    super::presets::capture_read(&db.connection, &mut provider).await?;
    crate::providers::login::capture(&mut provider)?;
    if provider.authentication != Authentication::Plugin {
        if mutation || !provider.enabled {
            return Err(denied());
        }
        let info = crate::store::plugins::get(&db.connection, &context.package.name)
            .await?
            .filter(|info| info.summary.available() && info.summary.reference() == context.package)
            .ok_or_else(denied)?;
        let resource = info
            .extension
            .as_ref()
            .and_then(|extension| {
                extension
                    .model_resources
                    .iter()
                    .find(|resource| resource.authentication == provider.authentication)
            })
            .filter(|resource| resource.valid())
            .ok_or_else(denied)?;
        let endpoint = resource.requests.get(name).ok_or_else(|| {
            Fault::new(
                ErrorCode::NotConfigured,
                "provider resource is not declared",
            )
        })?;
        if !crate::providers::login::resource_endpoint(provider.authentication, &endpoint.url) {
            return Err(denied());
        }
        return prepare_endpoint(endpoint, input, provider, Some((&context.package, name)));
    }
    let binding = provider.binding.as_ref().ok_or_else(denied)?;
    let package = binding.package.as_ref().ok_or_else(denied)?;
    if package != &context.package || provider.authentication != Authentication::Plugin {
        return Err(denied());
    }
    super::consent::check(&provider, Scope::Credentials)?;
    // Live provider resources resolve the current package before durable admission.
    let info = crate::store::plugins::get(&db.connection, &package.name)
        .await?
        .filter(|info| info.summary.available())
        .ok_or_else(denied)?;
    if info.summary.reference() != *package {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "channel plugin configuration changed",
        ));
    }
    let flow = &binding.auth.as_ref().ok_or_else(denied)?.flow;
    let declaration = info
        .extension
        .as_ref()
        .and_then(|extension| extension.model_auth.iter().find(|item| &item.id == flow))
        .filter(|declaration| declaration.valid())
        .ok_or_else(denied)?;
    let declarations = if mutation {
        &declaration.mutations
    } else {
        &declaration.requests
    };
    let endpoint = declarations.get(name).ok_or_else(|| {
        Fault::new(
            ErrorCode::NotConfigured,
            "provider resource is not declared",
        )
    })?;
    prepare_endpoint(
        endpoint,
        input,
        provider,
        (!mutation).then_some((&context.package, name)),
    )
}

fn prepare_endpoint(
    endpoint: &plugin::model_auth::Endpoint,
    input: &std::collections::BTreeMap<String, String>,
    provider: Provider,
    cache: Option<(&plugin::Reference, &str)>,
) -> Result<crate::plugins::http::Input, Fault> {
    let fields = input
        .iter()
        .map(|(name, value)| {
            if !plugin::host::valid_handler(name) {
                return Err(Fault::invalid("provider resource input name is invalid"));
            }
            Ok((format!("input.{name}"), Secret::new(value.clone())))
        })
        .collect::<Result<_, Fault>>()?;
    let mut prepared = crate::plugins::http::prepare_provider(endpoint, &fields, provider)?;
    if let Some((package, name)) = cache {
        crate::plugins::http::provider::cache(&mut prepared, package, name, endpoint.cache_ttl_ms)?;
    }
    Ok(prepared)
}

fn denied() -> Fault {
    Fault::new(
        ErrorCode::PermissionDenied,
        "provider resource does not match the calling plugin",
    )
}

impl Ingress {
    pub(in crate::store) async fn request_provider_http(
        &self,
        caller: NodeId,
        request: Request,
    ) -> Result<Admission, Fault> {
        let (reply, response) = oneshot::channel();
        self.send_internal(Job::ProviderHttp {
            caller,
            request: Box::new(request.clone()),
            reply,
        })
        .await?;
        let input = tokio::select! {
            biased;
            _ = self.closed.cancelled() => return Err(unavailable()),
            response = response => response.map_err(|_|unavailable())??,
        };
        let response = crate::plugins::http::run(*input, self, self.closed.child_token()).await?;
        let (reply, completion) = oneshot::channel();
        let _ = reply.send(Ok(Output::PluginHttp(response)));
        Ok(Admission {
            receipt: Receipt {
                id: request.id,
                durable: false,
            },
            completion,
        })
    }
}
