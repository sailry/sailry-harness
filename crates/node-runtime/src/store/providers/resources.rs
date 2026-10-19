//! Package-declared provider reads retain the existing authorization and HTTP owners.
use super::*;
use crate::store::database::Database;
use sailry_protocol::{conversation::consent::Scope, plugin};

pub(in crate::store) fn prepare(
    db: &Database,
    caller: NodeId,
    request: &Request,
) -> Result<crate::plugins::http::Input, Fault> {
    db.check_plugin(caller, request)?;
    let context = request.plugin.as_ref().ok_or_else(denied)?;
    let Command::RequestProviderHttp {
        provider,
        expected_revision,
        request: name,
        input,
    } = &request.command
    else {
        return Err(Fault::invalid("provider resource read expected"));
    };
    let mut provider = super::current(&db.connection, *provider, *expected_revision)?;
    super::presets::capture_read(&db.connection, &mut provider)?;
    crate::providers::login::capture(&mut provider)?;
    let binding = provider.binding.as_ref().ok_or_else(denied)?;
    let package = binding.package.as_ref().ok_or_else(denied)?;
    if package != &context.package || provider.authentication != Authentication::Plugin {
        return Err(denied());
    }
    super::consent::check(&provider, Scope::Credentials)?;
    // These are live, explicitly selected provider reads, not historical tool execution.
    let info = crate::store::plugins::get(&db.connection, &package.name)?
        .filter(|info| info.summary.enabled)
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
    let endpoint = declaration.requests.get(name).ok_or_else(|| {
        Fault::new(
            ErrorCode::NotConfigured,
            "provider resource is not declared",
        )
    })?;
    let fields = input
        .iter()
        .map(|(name, value)| {
            if !plugin::host::valid_handler(name) {
                return Err(Fault::invalid("provider resource input name is invalid"));
            }
            Ok((format!("input.{name}"), Secret::new(value.clone())))
        })
        .collect::<Result<_, Fault>>()?;
    crate::plugins::http::prepare_provider(endpoint, &fields, provider)
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
        if request.target != self.node || request.version != VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
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
