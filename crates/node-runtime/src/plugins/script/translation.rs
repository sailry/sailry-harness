//! Pure package transformations. A blocking owner retains one VM for a stream.
//! No SDK dispatch, logging, host modules, I/O or credentials are installed.
use super::{Bundle, CALL_TIMEOUT, MEMORY_BYTES, STACK_BYTES};
use rquickjs::{
    Context, Function, Module, Object, Persistent, Runtime, Value,
    loader::{BuiltinLoader, BuiltinResolver},
};
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[cfg(test)]
mod tests;

/// Persistent JS values must drop before their context and runtime.
pub(crate) struct Session {
    exports: Persistent<Object<'static>>,
    context: Context,
    _runtime: Runtime,
    deadline: Arc<Mutex<Option<Instant>>>,
    stop: CancellationToken,
    timeout: Duration,
    failed: bool,
}

impl Session {
    /// `bundle` must come from the captured immutable package resource owner.
    pub(crate) fn new(bundle: Bundle, stop: CancellationToken) -> Result<Self, Fault> {
        Self::bounded(bundle, stop, CALL_TIMEOUT)
    }

    pub(crate) fn has_export(&self, handler: &str) -> Result<bool, Fault> {
        check(&self.stop)?;
        self.context.with(|ctx| {
            let exports = self
                .exports
                .clone()
                .restore(&ctx)
                .map_err(|_| unavailable())?;
            let value: Value = exports.get(handler).map_err(|_| unavailable())?;
            if value.is_undefined() {
                Ok(false)
            } else if value.is_function() {
                Ok(true)
            } else {
                Err(unavailable())
            }
        })
    }

    fn bounded(bundle: Bundle, stop: CancellationToken, timeout: Duration) -> Result<Self, Fault> {
        check(&stop)?;
        let runtime = Runtime::new().map_err(|_| unavailable())?;
        runtime.set_memory_limit(MEMORY_BYTES);
        runtime.set_max_stack_size(STACK_BYTES);
        let deadline = Arc::new(Mutex::new(Some(Instant::now() + timeout)));
        let interrupted = stop.clone();
        let expires = deadline.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || {
            interrupted.is_cancelled()
                || expires.lock().map_or(true, |deadline| {
                    deadline.is_some_and(|deadline| Instant::now() >= deadline)
                })
        })));
        let mut resolver = BuiltinResolver::default();
        let mut loader = BuiltinLoader::default();
        resolver.add_module("sailry/adapter");
        loader.add_module("sailry/adapter", include_str!("adapter.js"));
        let mut files = bundle.files;
        let source = files
            .remove(&bundle.entry)
            .ok_or_else(|| Fault::invalid("plugin translation entry is missing"))?;
        for (name, source) in files {
            // Only package resources are registered. In particular, a package
            // cannot install a replacement for a host or standard-library module.
            resource(&name)?;
            resolver.add_module(name.clone());
            loader.add_module(name, source);
        }
        resource(&bundle.entry)?;
        resolver.add_module(bundle.entry.clone());
        runtime.set_loader(resolver, loader);
        let context = Context::full(&runtime).map_err(|_| unavailable())?;
        let exports = context.with(|ctx| {
            let (module, ready) = Module::declare(ctx.clone(), bundle.entry, source)?.eval()?;
            ready.finish::<()>()?;
            Ok::<_, rquickjs::Error>(Persistent::save(&ctx, module.namespace()?))
        });
        check(&stop)?;
        if deadline
            .lock()
            .map_err(|_| unavailable())?
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err(expired());
        }
        *deadline.lock().map_err(|_| unavailable())? = None;
        Ok(Self {
            exports: exports.map_err(|_| unavailable())?,
            context,
            _runtime: runtime,
            deadline,
            stop,
            timeout,
            failed: false,
        })
    }

    /// Uses the caller's existing request/result byte budget, without truncation.
    /// Errors intentionally exclude thrown messages and values, including tokens
    /// passed to an explicitly consented authentication exchange.
    pub(crate) fn call(
        &mut self,
        handler: &str,
        input: &serde_json::Value,
        max_bytes: usize,
    ) -> Result<serde_json::Value, Fault> {
        check(&self.stop)?;
        if self.failed {
            return Err(unavailable());
        }
        let input = serde_json::to_string(input).map_err(|_| unavailable())?;
        if input.len() > max_bytes {
            return Err(Fault::invalid(
                "plugin translation input exceeds its byte budget",
            ));
        }
        *self.deadline.lock().map_err(|_| unavailable())? = Some(Instant::now() + self.timeout);
        let result = self.context.with(|ctx| {
            let exports = self.exports.clone().restore(&ctx)?;
            let handler: Function = exports.get(handler)?;
            let input = ctx.json_parse(input)?;
            let value: Value = handler.call((input,))?;
            let value = match value.as_promise() {
                Some(promise) => promise.finish::<Value>()?,
                None => value,
            };
            ctx.json_stringify(value)?
                .ok_or(rquickjs::Error::Unknown)?
                .to_string()
        });
        let result = (|| {
            check(&self.stop)?;
            if self
                .deadline
                .lock()
                .map_err(|_| unavailable())?
                .is_some_and(|deadline| Instant::now() >= deadline)
            {
                return Err(expired());
            }
            let text = result.map_err(|_| unavailable())?;
            if text.len() > max_bytes {
                return Err(Fault::invalid(
                    "plugin translation result exceeds its byte budget",
                ));
            }
            serde_json::from_str(&text).map_err(|_| unavailable())
        })();
        *self.deadline.lock().map_err(|_| unavailable())? = None;
        self.failed = result.is_err();
        result
    }
}

fn resource(path: &str) -> Result<(), Fault> {
    if !path.starts_with("dev.sailry.platform/")
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || path.contains('\\')
    {
        return Err(Fault::invalid(
            "plugin translation resource is outside its namespace",
        ));
    }
    Ok(())
}

fn check(stop: &CancellationToken) -> Result<(), Fault> {
    if stop.is_cancelled() {
        return Err(Fault::new(
            ErrorCode::Cancelled,
            "plugin translation cancelled",
        ));
    }
    Ok(())
}

fn unavailable() -> Fault {
    Fault::invalid("plugin translation failed")
}

fn expired() -> Fault {
    Fault::new(ErrorCode::Cancelled, "plugin translation deadline exceeded")
}
