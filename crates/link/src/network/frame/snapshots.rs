use iroh::endpoint::SendStream;
use sailry_protocol::{ErrorCode, Fault, NodeId, Output, Snapshot, Update};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::{Reply, invalid};

// A JSON string uses at most six bytes per payload byte. Leave ample room
// for its envelope without changing the protocol's per-frame limit.
const PAGE_BYTES: usize = sailry_protocol::MAX_FRAME_BYTES / 16;

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Delivery {
    Completed,
    Update,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Header {
    delivery: Delivery,
    node: NodeId,
    cursor: u64,
    bytes: u64,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Page {
    index: u64,
    data: String,
}

pub(super) async fn write(
    core: &super::super::Core,
    stream: &mut SendStream,
    reply: &Reply,
    compressed: bool,
) -> Result<(), Fault> {
    let (snapshot, delivery) = match reply {
        Reply::Completed(response) => match response.as_ref() {
            Ok(Output::Snapshot(snapshot)) => (snapshot, Delivery::Completed),
            _ => {
                return core
                    .wait(super::write_with(stream, reply, compressed))
                    .await?;
            }
        },
        Reply::Update(update) => match update.as_ref() {
            Update::Snapshot(snapshot) => (snapshot, Delivery::Update),
            _ => {
                return core
                    .wait(super::write_with(stream, reply, compressed))
                    .await?;
            }
        },
        _ => {
            return core
                .wait(super::write_with(stream, reply, compressed))
                .await?;
        }
    };
    // Serialize the already captured service result once. Pages never query the
    // live store again, so all records and the cursor belong to one snapshot.
    let bytes = Zeroizing::new(serde_json::to_string(snapshot).map_err(invalid)?);
    let header = Header {
        delivery,
        node: snapshot.node,
        cursor: snapshot.cursor,
        bytes: u64::try_from(bytes.len()).map_err(invalid)?,
    };
    core.wait(super::write_with(
        stream,
        &Reply::SnapshotStart(header),
        compressed,
    ))
    .await??;
    let mut start = 0;
    let mut index = 0;
    while start < bytes.len() {
        let mut end = (start + PAGE_BYTES).min(bytes.len());
        while !bytes.is_char_boundary(end) {
            end -= 1;
        }
        // Each bounded page has the ordinary frame deadline; the complete
        // snapshot can take longer without imposing a business-operation limit.
        core.wait(super::write_with(
            stream,
            &Reply::SnapshotPage(Page {
                index,
                data: bytes[start..end].to_owned(),
            }),
            compressed,
        ))
        .await??;
        start = end;
        index += 1;
    }
    Ok(())
}

#[derive(Default)]
pub(super) struct Assembly {
    pending: Option<Pending>,
}

struct Pending {
    header: Header,
    bytes: Zeroizing<Vec<u8>>,
    index: u64,
}

impl Assembly {
    pub(super) fn accept(&mut self, reply: Reply) -> Result<Option<Reply>, Fault> {
        match reply {
            Reply::SnapshotStart(header) => {
                if self.pending.is_some() || header.bytes == 0 {
                    return Err(invalid("unexpected snapshot start"));
                }
                // Never allocate from a peer's declared total size. Grow only
                // after receiving and validating a bounded page.
                self.pending = Some(Pending {
                    header,
                    bytes: Zeroizing::new(Vec::new()),
                    index: 0,
                });
                Ok(None)
            }
            Reply::SnapshotPage(page) => {
                let Some(pending) = self.pending.as_mut() else {
                    return Err(invalid("snapshot start expected"));
                };
                let length = pending
                    .bytes
                    .len()
                    .checked_add(page.data.len())
                    .ok_or_else(|| invalid("snapshot size overflow"))?;
                if page.index != pending.index
                    || page.data.is_empty()
                    || page.data.len() > PAGE_BYTES
                    || u64::try_from(length).map_err(invalid)? > pending.header.bytes
                {
                    return Err(invalid("invalid snapshot page"));
                }
                pending
                    .bytes
                    .try_reserve(page.data.len())
                    .map_err(|_| Fault::new(ErrorCode::Busy, "snapshot capacity is unavailable"))?;
                pending.bytes.extend_from_slice(page.data.as_bytes());
                pending.index = pending
                    .index
                    .checked_add(1)
                    .ok_or_else(|| invalid("snapshot page index overflow"))?;
                if u64::try_from(length).map_err(invalid)? != pending.header.bytes {
                    return Ok(None);
                }
                let Pending { header, bytes, .. } =
                    self.pending.take().expect("snapshot assembly exists");
                let snapshot: Snapshot = serde_json::from_slice(&bytes).map_err(invalid)?;
                if snapshot.node != header.node || snapshot.cursor != header.cursor {
                    return Err(invalid("snapshot identity does not match its pages"));
                }
                Ok(Some(match header.delivery {
                    Delivery::Completed => {
                        Reply::Completed(Box::new(Ok(Output::Snapshot(snapshot))))
                    }
                    Delivery::Update => Reply::Update(Box::new(Update::Snapshot(snapshot))),
                }))
            }
            reply => {
                if self.pending.is_some() {
                    Err(invalid("snapshot page expected"))
                } else {
                    Ok(Some(reply))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
