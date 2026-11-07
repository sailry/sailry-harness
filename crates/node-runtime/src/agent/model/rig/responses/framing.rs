use futures::StreamExt;
use rig_core::http_client::{BoxedStream, Bytes, Error, framing::NdjsonFramer};
use std::collections::VecDeque;

pub(in crate::agent::model::rig) fn stream(body: BoxedStream) -> BoxedStream {
    Box::pin(futures::stream::try_unfold(
        (Some(body), NdjsonFramer::new(), VecDeque::<Vec<u8>>::new()),
        |(mut body, mut lines, mut pending)| async move {
            loop {
                if let Some(line) = pending.pop_front() {
                    if let Some(frame) = event(&line)? {
                        return Ok(Some((frame, (body, lines, pending))));
                    }
                    continue;
                }
                let Some(source) = body.as_mut() else {
                    return Ok(None);
                };
                match source.next().await {
                    Some(chunk) => pending.extend(lines.push(&chunk?)),
                    None => {
                        body = None;
                        pending.extend(lines.finish());
                    }
                }
            }
        },
    ))
}

fn event(line: &[u8]) -> Result<Option<Bytes>, Error> {
    let line = std::str::from_utf8(line).map_err(Error::instance)?;
    if line.trim().is_empty() {
        return Ok(None);
    }
    let payload = if let Some(data) = line.strip_prefix("data:") {
        data.strip_prefix(' ').unwrap_or(data)
    } else {
        let field = line.split_once(':').map_or(line, |(field, _)| field);
        if field
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Ok(None);
        }
        line
    };
    Ok(Some(format!("data: {payload}\n\n").into()))
}
