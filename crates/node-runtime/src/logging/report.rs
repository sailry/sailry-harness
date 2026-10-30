//! Opt-in, bounded reports through Sentry's envelope protocol.
use crate::telemetry::Control;
use sailry_protocol::Fault;
use sentry::{Client, ClientOptions, Hub};
use std::{sync::Arc, time::Duration};
use tracing::{Level, Subscriber};
use tracing_subscriber::{Layer, filter::filter_fn, layer::Context, registry::LookupSpan};

mod diagnostics;
mod panics;
mod privacy;
#[cfg(test)]
mod tests;
mod transport;

pub(crate) struct Report {
    client: Arc<Client>,
    transport: Arc<transport::Network>,
    control: Arc<Control>,
}

impl Report {
    pub(crate) fn new(control: Arc<Control>, dsn: Option<&str>) -> Option<Self> {
        let report = Self::create(control, dsn)?;
        // New worker threads inherit this client. No thread-local initialization
        // is needed for panics or structured application events.
        Hub::main().bind_client(Some(report.client.clone()));
        panics::install(&report.client);
        Some(report)
    }

    fn create(control: Arc<Control>, dsn: Option<&str>) -> Option<Self> {
        let dsn = dsn?.parse::<sentry::types::Dsn>().ok()?;
        let network = Arc::new(transport::Network::new(control.clone(), &dsn)?);
        let limiter = Arc::new(privacy::Limiter::default());
        let automatic = control.clone();
        let breadcrumbs = control.clone();
        let mut options = ClientOptions::default();
        options.dsn = Some(dsn);
        let options = options
            .transport(network.clone())
            .default_integrations(false)
            .add_integration(sentry::integrations::backtrace::AttachStacktraceIntegration)
            .add_integration(sentry::integrations::backtrace::ProcessStacktraceIntegration)
            .add_integration(sentry::integrations::debug_images::DebugImagesIntegration::new())
            .add_integration(sentry::integrations::contexts::ContextIntegration::new())
            .add_integration(sentry::integrations::panic::PanicIntegration::new())
            .attach_stacktrace(true)
            .send_default_pii(false)
            .max_breadcrumbs(50)
            // Ordinary events use the worker queue. A crashing thread alone waits
            // for at most two seconds so fatal exit cannot discard its panic.
            .shutdown_timeout(Duration::from_secs(2))
            .before_send(move |mut event| {
                let epoch = automatic.epoch();
                if !automatic.permitted() || !automatic.errors() {
                    return None;
                }
                event.breadcrumbs.values.retain(|breadcrumb| {
                    breadcrumb
                        .data
                        .get("_telemetry_epoch")
                        .and_then(serde_json::Value::as_u64)
                        == Some(epoch)
                });
                let mut event = privacy::event(event, &automatic.metadata(), false)?;
                if epoch != automatic.epoch() || !automatic.errors() || !limiter.admit(&event) {
                    return None;
                }
                event.extra.insert("_telemetry_epoch".into(), epoch.into());
                Some(event)
            })
            .before_breadcrumb(move |breadcrumb| {
                let epoch = breadcrumbs.epoch();
                if !breadcrumbs.errors() {
                    return None;
                }
                let mut breadcrumb = privacy::breadcrumb(breadcrumb)?;
                if epoch != breadcrumbs.epoch() || !breadcrumbs.errors() {
                    return None;
                }
                breadcrumb
                    .data
                    .insert("_telemetry_epoch".into(), epoch.into());
                Some(breadcrumb)
            });
        Some(Self {
            client: Arc::new(Client::from(options)),
            transport: network,
            control,
        })
    }

    pub(crate) fn layer<S>(&self) -> impl Layer<S> + use<S>
    where
        S: Subscriber + for<'a> LookupSpan<'a>,
    {
        Events {
            client: self.client.clone(),
            control: self.control.clone(),
        }
        .with_filter(filter_fn(|metadata| {
            metadata.target() == "sailry" && matches!(*metadata.level(), Level::ERROR | Level::WARN)
        }))
    }

    pub(crate) fn close(&self, timeout: Duration) -> bool {
        self.client.close(Some(timeout))
    }

    pub(crate) async fn diagnostics(&self) -> Result<(), Fault> {
        if !self.control.permitted() {
            return Err(diagnostics::failure());
        }
        let path = super::current_file().map_err(|_| diagnostics::failure())?;
        self.diagnostics_from(path).await
    }

    async fn diagnostics_from(&self, path: Option<std::path::PathBuf>) -> Result<(), Fault> {
        if !self.control.permitted() {
            return Err(diagnostics::failure());
        }
        let epoch = self.control.epoch();
        let attachment = tokio::task::spawn_blocking(move || diagnostics::attachment(path))
            .await
            .map_err(|_| diagnostics::failure())??;
        let event = privacy::event(
            sentry::protocol::Event::default(),
            &self.control.metadata(),
            true,
        )
        .ok_or_else(diagnostics::failure)?;
        let mut envelope: sentry::Envelope = event.into();
        envelope.add_item(attachment);
        self.transport
            .explicit(envelope, epoch)
            .await
            .map_err(|_| diagnostics::failure())
    }
}

struct Events {
    client: Arc<Client>,
    control: Arc<Control>,
}

impl<S> Layer<S> for Events
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &tracing::Event<'_>, context: Context<'_, S>) {
        if !self.control.errors() || event.metadata().target() != "sailry" {
            return;
        }
        let level = *event.metadata().level();
        if !matches!(level, Level::ERROR | Level::WARN) {
            return;
        }
        // A runtime worker may have initialized its Hub before logging started.
        // Binding the process client preserves that thread's bounded breadcrumbs.
        let hub = Hub::current();
        if !hub
            .client()
            .is_some_and(|client| Arc::ptr_eq(&client, &self.client))
        {
            hub.bind_client(Some(self.client.clone()));
        }
        if level == Level::ERROR {
            sentry_tracing::ErrorLayer::new().on_event(event, context);
        } else {
            sentry_tracing::BreadcrumbLayer::new().on_event(event, context);
        }
    }
}
