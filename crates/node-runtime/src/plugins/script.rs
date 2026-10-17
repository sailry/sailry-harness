//! Bounded headless execution using the same QuickJS binding as GPUI Shell.
//! No GPUI, filesystem loader, process, fetch, or Node.js globals are installed.
use rquickjs::{
    Context, Ctx, Function, Module, Runtime, Value,
    loader::{BuiltinLoader, BuiltinResolver},
    prelude::List,
};
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, plugin};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

pub(crate) mod execution;
#[cfg(feature = "test-support")]
pub mod fixture;
mod sdk;
#[cfg(test)]
mod tests;

pub(crate) struct Bundle {
    pub entry: String,
    pub files: BTreeMap<String, String>,
}

pub(crate) struct Environment {
    pub context: plugin::Context,
    pub target: sailry_protocol::NodeId,
    pub stop: CancellationToken,
}

pub(crate) fn run(
    bundle: Bundle,
    handler: &str,
    input: serde_json::Value,
    context: plugin::Context,
    target: sailry_protocol::NodeId,
    stop: CancellationToken,
    dispatch: impl FnMut(sailry_protocol::Request) -> sailry_link::Response + 'static,
) -> Result<serde_json::Value, Fault> {
    evaluate(
        bundle,
        handler,
        input,
        Environment {
            context,
            target,
            stop,
        },
        dispatch,
        Duration::from_secs(60),
        None,
    )
}

/// Executes a tool with invocation-local policy and ordinary durable SDK writes.
pub(crate) fn run_tool(
    bundle: Bundle,
    handler: &str,
    input: serde_json::Value,
    environment: Environment,
    dispatch: impl FnMut(sailry_protocol::Request) -> sailry_link::Response + 'static,
    execution: std::sync::Arc<std::sync::Mutex<execution::State>>,
) -> Result<serde_json::Value, Fault> {
    evaluate(
        bundle,
        handler,
        input,
        environment,
        dispatch,
        Duration::from_secs(60),
        Some(execution),
    )
}

fn evaluate(
    bundle: Bundle,
    handler: &str,
    input: serde_json::Value,
    environment: Environment,
    dispatch: impl FnMut(sailry_protocol::Request) -> sailry_link::Response + 'static,
    timeout: Duration,
    execution: Option<std::sync::Arc<std::sync::Mutex<execution::State>>>,
) -> Result<serde_json::Value, Fault> {
    let context = &environment.context;
    let mut entry =
        crate::logging::Entry::new(crate::logging::Area::Plugin, crate::logging::Phase::Begin);
    entry.context = crate::logging::Context {
        node: Some(environment.target),
        package: Some(context.package.clone()),
        session: context.session,
        turn: context.turn,
        worktree: context.worktree,
        invocation: context.invocation,
        handler: Some(handler.into()),
        call: execution
            .as_ref()
            .and_then(|state| state.lock().ok().and_then(|state| state.call_id)),
        ..Default::default()
    };
    let mut span = crate::logging::Span::new(entry).cancel_when(environment.stop.clone());
    let result = evaluate_with(
        bundle,
        handler,
        input,
        environment,
        dispatch,
        timeout,
        execution,
    );
    match &result {
        Ok(_) => span.finish(crate::logging::Phase::Completed, None),
        Err(fault) => {
            span.entry.reason = Some("callback");
            span.finish(
                if fault.code == ErrorCode::Cancelled {
                    crate::logging::Phase::Cancelled
                } else {
                    crate::logging::Phase::Failed
                },
                Some(fault.code),
            );
        }
    }
    result
}

fn evaluate_with(
    bundle: Bundle,
    handler: &str,
    input: serde_json::Value,
    environment: Environment,
    dispatch: impl FnMut(sailry_protocol::Request) -> sailry_link::Response + 'static,
    timeout: Duration,
    execution: Option<std::sync::Arc<std::sync::Mutex<execution::State>>>,
) -> Result<serde_json::Value, Fault> {
    let Environment {
        context,
        target,
        stop,
    } = environment;
    let deadline = Instant::now() + timeout;
    let runtime = Runtime::new().map_err(engine)?;
    runtime.set_memory_limit(64 * 1024 * 1024);
    runtime.set_max_stack_size(512 * 1024);
    let interrupted = stop.clone();
    runtime.set_interrupt_handler(Some(Box::new(move || {
        interrupted.is_cancelled() || Instant::now() >= deadline
    })));
    let mut resolver = BuiltinResolver::default()
        .with_module("sailry/sdk")
        .with_module("sailry/models")
        .with_module("sailry/cards")
        .with_module("sailry/installations");
    let mut loader = BuiltinLoader::default()
        .with_module("sailry/sdk", include_str!("script/sdk.js"))
        .with_module("sailry/models", include_str!("script/models.js"))
        .with_module("sailry/cards", include_str!("script/cards.js"))
        .with_module(
            "sailry/installations",
            include_str!("script/installations.js"),
        );
    let mut files = bundle.files;
    let source = files
        .remove(&bundle.entry)
        .ok_or_else(|| sailry_protocol::Fault::invalid("plugin host entry is missing"))?;
    for (name, source) in files {
        resolver.add_module(name.clone());
        loader.add_module(name, source);
    }
    resolver.add_module(bundle.entry.clone());
    runtime.set_loader(resolver, loader);
    let vm = Context::full(&runtime).map_err(engine)?;
    let result = vm.with(|ctx| -> Result<String, Fault> {
        // Capture intrinsics before package code can replace them. Only own string
        // data properties of native Errors are inspected; accessors never run.
        let inspect: Function = ctx
            .eval(
                r#"(() => {
            const descriptor = Object.getOwnPropertyDescriptor;
            const own = Object.hasOwn;
            const field = (error, key, limit) => {
                const property = descriptor(error, key);
                return property && own(property, 'value')
                    && typeof property.value === 'string' && property.value.length <= limit
                    ? property.value : null;
            };
            return error => [field(error, 'message', 1024), field(error, 'code', 32)];
        })()"#,
            )
            .map_err(engine)?;
        let result = (|| -> rquickjs::Result<String> {
            sdk::install(
                &ctx,
                target,
                context,
                stop.clone(),
                deadline,
                dispatch,
                execution,
            )?;
            let (module, ready) = Module::declare(ctx.clone(), bundle.entry, source)?.eval()?;
            ready.finish::<()>()?;
            let function: Function = module.get(handler)?;
            let input = ctx.json_parse(input.to_string())?;
            let value: Value = function.call((input,))?;
            let value = match value.as_promise() {
                Some(promise) => promise.finish::<Value>()?,
                None => value,
            };
            ctx.json_stringify(value)?
                .ok_or(rquickjs::Error::Unknown)?
                .to_string()
        })();
        result.map_err(|error| exception(&ctx, &inspect, error))
    });
    if stop.is_cancelled() {
        return Err(Fault::new(
            ErrorCode::Cancelled,
            "plugin callback cancelled",
        ));
    }
    if Instant::now() >= deadline {
        return Err(Fault::new(
            ErrorCode::Cancelled,
            "plugin callback deadline exceeded",
        ));
    }
    let text = result?;
    if text.len() > plugin::host::MAX_DATA_BYTES {
        return Err(sailry_protocol::Fault::invalid(
            "plugin callback result exceeds limit",
        ));
    }
    serde_json::from_str(&text)
        .map_err(|_| sailry_protocol::Fault::invalid("plugin callback result is not JSON"))
}

fn engine(error: rquickjs::Error) -> Fault {
    // Never stringify a thrown package object: getters may run unbounded user code.
    Fault::invalid(format!("plugin JavaScript execution failed: {error}"))
}

fn exception<'js>(ctx: &Ctx<'js>, inspect: &Function<'js>, error: rquickjs::Error) -> Fault {
    if !matches!(error, rquickjs::Error::Exception) {
        return engine(error);
    }
    let thrown = ctx.catch();
    if !thrown.is_error() {
        return engine(error);
    }
    let fields: Result<List<(Option<String>, Option<String>)>, _> = inspect.call((thrown,));
    let Ok(List((Some(message), code))) = fields else {
        return engine(error);
    };
    if message.is_empty() || message.len() > 1024 {
        return engine(error);
    }
    let code = code
        .and_then(|code| serde_json::from_value(serde_json::Value::String(code)).ok())
        .unwrap_or(ErrorCode::InvalidRequest);
    Fault::new(
        code,
        format!("plugin JavaScript execution failed: {message}"),
    )
}
