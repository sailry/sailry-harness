use super::*;
use crate::dictation::permission;

fn status() -> Status {
    match permission::status() {
        permission::Status::Granted => Status::Granted,
        permission::Status::NotDetermined => Status::Required,
        permission::Status::Denied => Status::Denied,
        permission::Status::Restricted => Status::Restricted,
        permission::Status::Unknown => Status::Unknown,
    }
}
pub(crate) fn open(stop: CancellationToken, done: Completion, window: &mut Window, cx: &mut App) {
    open_with_status(status(), stop, done, window, cx);
}

pub(super) fn open_with_status(
    initial: Status,
    stop: CancellationToken,
    done: Completion,
    window: &mut Window,
    cx: &mut App,
) {
    if !cfg!(target_os = "macos") || initial == Status::Granted {
        window.defer(cx, move |window, cx| done(!stop.is_cancelled(), window, cx));
        return;
    }
    let check: Action = Rc::new(|cx, _| {
        cx.background_executor()
            .spawn(async { Ok(vec![(Resource::Microphone, status())]) })
    });
    let request: Action = Rc::new(|cx, stop| {
        cx.background_executor().spawn(async move {
            match permission::ensure(&stop).await {
                Ok(()) => Ok(vec![(Resource::Microphone, status())]),
                Err(key) => Err(Failure {
                    key: key.into(),
                    status: status(),
                }),
            }
        })
    });
    super::open(
        vec![Card {
            resource: Resource::Microphone,
            status: initial,
            settings: permission::settings_url(),
            check: Some(check),
            request: Some(request),
            requires: None,
        }],
        stop,
        done,
        window,
        cx,
    );
}
