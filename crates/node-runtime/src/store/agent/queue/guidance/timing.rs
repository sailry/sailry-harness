use super::*;
use std::future::Future;

pub(in crate::store::agent) fn measure<T>(
    request: &Request,
    session: SessionId,
    stage: &'static str,
    operation: impl Future<Output = Result<T, Fault>>,
) -> impl Future<Output = Result<T, Fault>> {
    let mut span = matches!(request.command, Command::GuideTurn { .. }).then(|| {
        let mut entry =
            crate::logging::Entry::new(crate::logging::Area::Command, crate::logging::Phase::Begin);
        entry.command = Some("guide_turn");
        entry.stage = Some(stage);
        entry.context.node = Some(request.target);
        entry.context.request = Some(request.id);
        entry.context.session = Some(session);
        crate::logging::Span::new(entry)
    });
    let operation = Box::pin(operation);
    async move {
        let result = operation.await;
        if let Some(span) = &mut span {
            span.finish(
                if result.is_ok() {
                    crate::logging::Phase::Completed
                } else {
                    crate::logging::Phase::Failed
                },
                result.as_ref().err().map(|fault| fault.code),
            );
        }
        result
    }
}
