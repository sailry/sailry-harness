use super::*;
use sailry_protocol::{Event, Update};

impl Panel {
    pub(super) fn watch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.stop.is_some() {
            return;
        }
        self.generation += 1;
        let generation = self.generation;
        let stop = CancellationToken::new();
        self.stop = Some(stop.clone());
        let binding = self.binding.clone();
        let (sender, mut receiver) = tokio::sync::mpsc::channel(8);
        self.binding.runtime.spawn(async move {
            loop {
                let stream = tokio::select! { _ = stop.cancelled() => return, stream = binding.client.subscribe() => stream };
                if let Ok(mut stream) = stream {
                    loop {
                        let update = tokio::select! { _ = stop.cancelled() => return, update = stream.next() => update };
                        let relevant = match update {
                            Ok(Update::Event(envelope)) => matches!(envelope.event, Event::BackupSettingsChanged { .. }),
                            Ok(Update::Snapshot(_)) | Ok(Update::ResetRequired) => true,
                            Ok(_) => false, Err(_) => break,
                        };
                        if relevant { tokio::select! { _ = stop.cancelled() => return, sent = sender.send(()) => if sent.is_err() { return } } }
                    }
                }
                tokio::select! { _ = stop.cancelled() => return, _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => () }
            }
        });
        cx.spawn_in(window, async move |owner, cx| {
            while receiver.recv().await.is_some() {
                if owner
                    .update_in(cx, |panel, window, cx| {
                        if panel.active && panel.generation == generation {
                            panel.load(window, cx);
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }
}
