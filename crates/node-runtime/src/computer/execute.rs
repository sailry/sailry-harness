//! Public computer calls dispatch the original Cua tool and result.
use crate::store::Ingress;
use sailry_link::CancellationToken;
use sailry_protocol::{Command, ErrorCode, Fault, NodeId, Output, Request};
use serde_json::Value;

pub(crate) fn validate(name: &str, arguments: &Value) -> Result<(), Fault> {
    if super::definition(name).is_none() {
        return Err(Fault::invalid("unknown computer tool"));
    }
    let Some(arguments) = arguments.as_object() else {
        return Err(Fault::invalid("computer arguments must be an object"));
    };
    if let Some(name) = super::catalog::HIDDEN
        .iter()
        .find(|name| arguments.contains_key(**name))
    {
        return Err(Fault::invalid(format!(
            "computer argument {name} is not supported"
        )));
    }
    Ok(())
}
pub(crate) async fn run(
    ingress: &Ingress,
    caller: NodeId,
    request: &Request,
    stop: CancellationToken,
) -> Result<Output, Fault> {
    let Command::UseComputer {
        session,
        name,
        arguments,
        ..
    } = &request.command
    else {
        unreachable!()
    };
    validate(name, arguments)?;
    let read_only = super::read_only(name).expect("validated computer tool");
    ingress.check_plugin(caller, request.clone()).await?;
    let mut rejected = None;
    let mut started = false;
    let result = ingress
        .computer
        .execute(*session, name, arguments.clone(), &stop, async {
            let result = ingress
                .check_plugin(caller, request.clone())
                .await
                .map_err(|fault| {
                    let message = fault.message.clone();
                    rejected = Some(fault);
                    message
                });
            started = result.is_ok();
            result
        })
        .await;
    if let Some(fault) = rejected {
        return Err(fault);
    }
    let result = result.map_err(|message| {
        Fault::new(
            if started && !read_only {
                ErrorCode::OutcomeUnknown
            } else if stop.is_cancelled() {
                ErrorCode::Cancelled
            } else if read_only {
                ErrorCode::Unavailable
            } else {
                ErrorCode::OutcomeUnknown
            },
            message,
        )
    })?;
    Ok(Output::Computer(result))
}
