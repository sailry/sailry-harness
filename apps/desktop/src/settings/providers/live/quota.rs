use super::*;
use std::time::Duration;

pub(in crate::settings::providers) fn now() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

pub(super) fn refresh(owner: &mut Workspace, cx: &mut Context<Workspace>) {
    let Some(live) = &mut owner.provider_link else {
        return;
    };
    let now = now();
    let reset = owner
        .providers
        .channels
        .iter()
        .filter(|channel| channel.preset == Preset::ChatGpt)
        .filter_map(|channel| channel.quota.as_ref())
        .flat_map(|quota| [&quota.five_hour, &quota.seven_day])
        .filter_map(|window| window.as_ref()?.reset_at_ms)
        .filter(|reset| *reset > now)
        .min();
    if live.quota_reset == reset {
        return;
    }
    live.quota_clock = None;
    live.quota_reset = reset;
    if let Some(reset) = reset {
        let node = live.binding.client.target();
        let timer = cx
            .background_executor()
            .timer(Duration::from_millis(reset - now));
        live.quota_clock = Some(cx.spawn(async move |owner, cx| {
            timer.await;
            let _ = owner.update(cx, |owner, cx| {
                let Some(live) = &mut owner.provider_link else {
                    return;
                };
                if live.binding.client.target() != node || live.quota_reset != Some(reset) {
                    return;
                }
                live.quota_reset = None;
                live.quota_clock = None;
                refresh(owner, cx);
                cx.notify();
            });
        }));
    }
}
