use super::{Service, State, Update};
use crate::updater::{
    tests::fixture,
    transfer::{Progress, Staged},
};
use gpui_kit::TestAppContext;
use sailry_link::CancellationToken;

#[gpui_kit::test]
fn cancels_and_ignores_stale_results(cx: &mut TestAppContext) {
    cx.update(crate::preferences::init);
    let service = cx.update(Service::shared);
    cx.update(|cx| {
        service.update(cx, |service, cx| {
            let config = fixture::config();
            let manifest = fixture::signed(&[fixture::release()]);
            let selection = super::super::manifest::select(&manifest, &config)
                .unwrap()
                .unwrap();

            service.state = State::Checking;
            service.stop = CancellationToken::new();
            let stopped = service.stop.clone();
            let stale_check = service.advance();
            assert!(service.channel_changed(cx));
            assert!(stopped.is_cancelled());
            service.accept(
                stale_check,
                Update::Checked(Ok((config.clone(), Some(selection.clone())))),
                cx,
            );
            assert_eq!(service.state, State::Idle);
            assert!(service.config.is_none());
            assert!(service.selection.is_none());

            service.state = State::Downloading(Progress::Downloading {
                copied: 1,
                total: Some(2),
            });
            service.config = Some(config);
            service.selection = Some(selection.clone());
            service.stop = CancellationToken::new();
            let stopped = service.stop.clone();
            let stale_download = service.advance();
            let directory = tempfile::tempdir().unwrap();
            let archive = directory.path().join(&selection.release.name);
            let staged = Staged {
                directory,
                archive,
                selection,
            };
            assert!(service.channel_changed(cx));
            assert!(stopped.is_cancelled());
            service.accept(stale_download, Update::Downloaded(Ok(staged)), cx);
            assert_eq!(service.state, State::Idle);
            assert!(service.staged.is_none());
        });
    });
}
