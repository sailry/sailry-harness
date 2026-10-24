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
pub(crate) mod translation;

const MEMORY_BYTES: usize = 64 * 1024 * 1024;
const STACK_BYTES: usize = 512 * 1024;
pub(crate) const CALL_TIMEOUT: Duration = Duration::from_secs(60);

pub(crate) struct Bundle {
    pub entry: String,
    pub files: BTreeMap<String, String>,
}

pub(crate) struct Environment {
    pub context: plugin::Context,
    pub target: sailry_protocol::NodeId,
    pub stop: CancellationToken,
    pub deadline: Instant,
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) struct Wait {
    pub entered: tokio::sync::oneshot::Sender<sailry_protocol::Request>,
    pub release: CancellationToken,
    pub durable: bool,
}

struct Control {
    stop: CancellationToken,
    deadline: Instant,
    failure: std::sync::Arc<std::sync::Mutex<Option<Fault>>>,
    faults: std::sync::Arc<std::sync::Mutex<BTreeMap<String, Fault>>>,
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn run(
    bundle: Bundle,
    handler: &str,
    input: serde_json::Value,
    context: plugin::Context,
    target: sailry_protocol::NodeId,
    stop: CancellationToken,
    dispatch: impl FnMut(sailry_protocol::Request) -> sailry_link::Response + 'static,
) -> Result<serde_json::Value, Fault> {
    run_callback(
        bundle,
        handler,
        input,
        Environment {
            context,
            target,
            stop,
            deadline: Instant::now() + CALL_TIMEOUT,
        },
        false,
        dispatch,
    )
}

pub(crate) fn run_callback(
    bundle: Bundle,
    handler: &str,
    input: serde_json::Value,
    environment: Environment,
    read_only: bool,
    mut dispatch: impl FnMut(sailry_protocol::Request) -> sailry_link::Response + 'static,
) -> Result<serde_json::Value, Fault> {
    evaluate(
        bundle,
        handler,
        input,
        environment,
        move |request| {
            // Nondurable cancellation and dismissal are still mutations. Check
            // at dispatch as well as the SDK's existing durable-write policy.
            if read_only
                && (request.command.durable()
                    || matches!(
                        request.command,
                        sailry_protocol::Command::RequestComputerPermission { .. }
                            | sailry_protocol::Command::DismissInstallation { .. }
                            | sailry_protocol::Command::CancelWebFetch { .. }
                    ))
            {
                return Err(Fault::new(
                    sailry_protocol::ErrorCode::PermissionDenied,
                    "read-only callbacks cannot dispatch mutations",
                ));
            }
            dispatch(request)
        },
        read_only.then(|| {
            std::sync::Arc::new(std::sync::Mutex::new(execution::State {
                call_id: None,
                mode: sailry_protocol::WorkMode::Code,
                read_only: true,
                turn: None,
                staged_turn: None,
            }))
        }),
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
        Some(execution),
    )
}

fn evaluate(
    bundle: Bundle,
    handler: &str,
    input: serde_json::Value,
    environment: Environment,
    dispatch: impl FnMut(sailry_protocol::Request) -> sailry_link::Response + 'static,
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
    let started = Instant::now();
    let budget = environment.deadline.saturating_duration_since(started);
    let scope = environment.context.clone();
    let mut result = evaluate_with(bundle, handler, input, environment, dispatch, execution);
    if let Err(fault) = &mut result {
        use sailry_protocol::{
            FaultDetails, RequestId,
            notification::{DiagnosticReason, SystemDiagnostic, SystemSource},
        };
        if fault.details.is_none() {
            let mut report =
                SystemDiagnostic::new(RequestId::new(), SystemSource::PluginCallback, fault.code);
            report.reason = match fault.code {
                ErrorCode::Cancelled => DiagnosticReason::Cancelled,
                ErrorCode::Expired => DiagnosticReason::Timeout,
                _ => DiagnosticReason::Failure,
            };
            fault.details = Some(FaultDetails::SystemDiagnostic(Box::new(report)));
        }
        if let Some(FaultDetails::SystemDiagnostic(report)) = &mut fault.details {
            report.session = scope.session;
            report.turn = scope.turn;
            report.worktree = scope.worktree;
            report.budget_ms = Some(budget.as_millis().min(u64::MAX as u128) as u64);
            report.elapsed_ms = Some(started.elapsed().as_millis().min(u64::MAX as u128) as u64);
        }
    }
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
    execution: Option<std::sync::Arc<std::sync::Mutex<execution::State>>>,
) -> Result<serde_json::Value, Fault> {
    let Environment {
        context,
        target,
        stop,
        deadline,
    } = environment;
    let failure = std::sync::Arc::new(std::sync::Mutex::new(None));
    let faults = std::sync::Arc::new(std::sync::Mutex::new(BTreeMap::new()));
    let runtime = Runtime::new().map_err(engine)?;
    runtime.set_memory_limit(MEMORY_BYTES);
    runtime.set_max_stack_size(STACK_BYTES);
    let interrupted = stop.clone();
    runtime.set_interrupt_handler(Some(Box::new(move || {
        interrupted.is_cancelled() || Instant::now() >= deadline
    })));
    let mut resolver = BuiltinResolver::default()
        .with_module("sailry/sdk")
        .with_module("sailry/http")
        .with_module("sailry/models")
        .with_module("sailry/decisions")
        .with_module("sailry/connectors")
        .with_module("sailry/cards")
        .with_module("sailry/installations");
    let mut loader = BuiltinLoader::default()
        .with_module("sailry/sdk", include_str!("script/sdk.js"))
        .with_module("sailry/http", include_str!("script/http.js"))
        .with_module("sailry/models", include_str!("script/models.js"))
        .with_module("sailry/decisions", include_str!("script/decisions.js"))
        .with_module("sailry/connectors", include_str!("script/connectors.js"))
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
            const prototype = Object.getPrototypeOf;
            const kinds = [['Error','error'],['TypeError','type_error'],['RangeError','range_error'],
                ['ReferenceError','reference_error'],['SyntaxError','syntax_error'],['URIError','uri_error'],
                ['EvalError','eval_error'],['AggregateError','aggregate_error']]
                .filter(([name]) => typeof globalThis[name] === 'function')
                .map(([name, kind]) => [globalThis[name].prototype, kind]);
            const classify = error => {
                let current = prototype(error);
                for (let depth=0; current && depth<8; depth++, current=prototype(current)) {
                    for (let index=0; index<kinds.length; index++) {
                        if (current === kinds[index][0]) return kinds[index][1];
                    }
                }
                return 'unknown';
            };
            const field = (error, key, limit) => {
                const property = descriptor(error, key);
                return property && own(property, 'value')
                    && typeof property.value === 'string' && property.value.length <= limit
                    ? property.value : null;
            };
            return error => [field(error, 'message', 1024), field(error, 'code', 32), field(error, 'sailryFault', 36), classify(error)];
        })()"#,
            )
            .map_err(engine)?;
        let result = (|| -> rquickjs::Result<String> {
            sdk::install(
                &ctx,
                target,
                context,
                Control {
                    stop: stop.clone(),
                    deadline,
                    failure: failure.clone(),
                    faults: faults.clone(),
                },
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
        result.map_err(|error| exception(&ctx, &inspect, error, &faults))
    });
    // An interrupted VM must not turn a possibly admitted write into a rejection.
    if let Some(fault) = failure.lock().expect("script result lock is valid").take() {
        return Err(fault);
    }
    if stop.is_cancelled() {
        return Err(Fault::new(
            ErrorCode::Cancelled,
            "plugin callback cancelled",
        ));
    }
    if Instant::now() >= deadline {
        return Err(Fault::new(
            ErrorCode::Expired,
            "plugin callback response timed out",
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

type ExceptionFields = (Option<String>, Option<String>, Option<String>, String);

fn exception<'js>(
    ctx: &Ctx<'js>,
    inspect: &Function<'js>,
    error: rquickjs::Error,
    faults: &std::sync::Mutex<BTreeMap<String, Fault>>,
) -> Fault {
    if !matches!(error, rquickjs::Error::Exception) {
        return engine(error);
    }
    let thrown = ctx.catch();
    if !thrown.is_error() {
        return classified(
            engine(error),
            sailry_protocol::notification::ScriptException::ThrownValue,
        );
    }
    let fields: Result<List<ExceptionFields>, _> = inspect.call((thrown,));
    let Ok(List((message, code, identity, category))) = fields else {
        return engine(error);
    };
    // A package can copy an opaque token, but cannot forge a host-authored Fault.
    if let Some(fault) = identity.and_then(|id| faults.lock().ok()?.remove(&id)) {
        return fault;
    }
    let category = serde_json::from_value(serde_json::Value::String(category))
        .unwrap_or(sailry_protocol::notification::ScriptException::Unknown);
    let Some(message) = message else {
        return classified(engine(error), category);
    };
    if message.is_empty() || message.len() > 1024 {
        return classified(engine(error), category);
    }
    let code = code
        .and_then(|code| serde_json::from_value(serde_json::Value::String(code)).ok())
        .unwrap_or(ErrorCode::InvalidRequest);
    classified(
        Fault::new(
            code,
            format!("plugin JavaScript execution failed: {message}"),
        ),
        category,
    )
}

fn classified(mut fault: Fault, category: sailry_protocol::notification::ScriptException) -> Fault {
    use sailry_protocol::{
        FaultDetails, RequestId,
        notification::{SystemDiagnostic, SystemSource},
    };
    let mut report =
        SystemDiagnostic::new(RequestId::new(), SystemSource::PluginCallback, fault.code);
    report.exception = Some(category);
    fault.details = Some(FaultDetails::SystemDiagnostic(Box::new(report)));
    fault
}
