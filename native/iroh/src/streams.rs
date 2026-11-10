use iroh::endpoint::VarInt;
use serde_json::json;
use std::sync::atomic::Ordering;

use crate::runtime::{Output, Owner, copy_input, error, spawn, state};

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sailry_iroh_send_write(
    send: u64,
    data: *const u8,
    length: usize,
) -> usize {
    let data = unsafe { copy_input(data, length) };
    spawn(state().send_owner(send), async move {
        let entry = state().send(send)?;
        let written = entry
            .stream
            .lock()
            .await
            .write(&data)
            .await
            .map_err(error)?;
        Ok(Output::Json(json!({ "written": written })))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn sailry_iroh_send_finish(send: u64) -> usize {
    spawn(state().send_owner(send), async move {
        let entry = state().send(send)?;
        entry.stream.lock().await.finish().map_err(error)?;
        entry.finished.store(true, Ordering::Release);
        Ok(Output::Empty)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn sailry_iroh_send_stopped(send: u64) -> usize {
    spawn(state().send_owner(send), async move {
        let entry = state().send(send)?;
        let stopped = entry.stream.lock().await.stopped();
        let code = stopped.await.map_err(error)?.map(VarInt::into_inner);
        Ok(Output::Json(json!({ "code": code })))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn sailry_iroh_send_reset(send: u64, code: u64) -> usize {
    let owner = Owner {
        direction: 0,
        ..state().send_owner(send)
    };
    spawn(owner, async move {
        state().interrupt_direction(send).await;
        let entry = state().send(send)?;
        let code = VarInt::from_u64(code).map_err(error)?;
        entry.stream.lock().await.reset(code).map_err(error)?;
        Ok(Output::Empty)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn sailry_iroh_recv_read(receive: u64, maximum: usize) -> usize {
    spawn(state().receive_owner(receive), async move {
        let entry = state().receive(receive)?;
        let mut bytes = vec![0; maximum];
        let length = entry
            .stream
            .lock()
            .await
            .read(&mut bytes)
            .await
            .map_err(error)?;
        match length {
            Some(length) => {
                bytes.truncate(length);
                Ok(Output::Bytes(bytes))
            }
            None => Ok(Output::Eof),
        }
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn sailry_iroh_recv_stop(receive: u64, code: u64) -> usize {
    let owner = Owner {
        direction: 0,
        ..state().receive_owner(receive)
    };
    spawn(owner, async move {
        state().interrupt_direction(receive).await;
        let entry = state().receive(receive)?;
        let code = VarInt::from_u64(code).map_err(error)?;
        entry.stream.lock().await.stop(code).map_err(error)?;
        Ok(Output::Empty)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn sailry_iroh_stream_drop(send: u64, receive: u64) -> usize {
    let owner = Owner {
        direction: 0,
        ..state().send_owner(send)
    };
    spawn(owner, async move {
        state().drop_streams(send, receive).await?;
        Ok(Output::Empty)
    })
}
