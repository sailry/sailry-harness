use super::*;

#[gpui::test]
fn distinguishes_first_read_and_refresh(cx: &mut TestAppContext) {
    init(cx);
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("main.js"),
        r#"
import {View,div} from 'gpui-kit';
import {CardList,CardRow,CardSummary} from 'sailry/ui';
export default class States extends View {
  render() {
    const props={empty_icon:'inbox',empty_label:'No items',skeleton_rows:1};
    return div().size_full().v_flex().p_4().gap_4().children([
      CardList.new('pending',{...props,loaded:false}),
      CardList.new('empty',{...props,loaded:true}),
      CardList.new('refresh',{...props,loaded:false})
        .child(CardRow.new('retained').child(CardSummary.new('summary',{title:'Item',subtitle:'Details'}))),
      CardList.new('layout'),
    ]);
  }
}
"#,
    )
    .unwrap();
    let policy = Policy::new()
        .with_capabilities(
            gpui_shell::Capabilities::new().read_roots([directory.path().to_path_buf()]),
        )
        .with_host_module(extend(HostModule::new("sailry/ui")))
        .unwrap();
    let mut previous = None;
    policy::update_default(|current| {
        previous = Some(current);
        policy
    });
    let _restore = DefaultPolicy(previous);
    let runtime = gpui_component_shell::new_isolated_runtime().unwrap();
    let mut application = None;
    let mut script = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = runtime.try_load(directory.path(), window, cx).unwrap();
        let content = view
            .read(cx)
            .content()
            .clone()
            .downcast::<gpui_shell::ScriptView>()
            .unwrap();
        script = Some(content.clone());
        application = Some(view);
        Root::new(content, window, cx)
    });
    let _application = application.unwrap();
    let script = script.unwrap();
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for width in [360., 960.] {
            let handle = visual.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                window.window_handle()
            });
            visual.simulate_window_resize(handle, size(px(width), px(640.)));
            visual.run_until_parked();
            visual.update(|window, cx| {
                window.draw(cx).clear(cx);
                assert_eq!(script.read(cx).build_error(), None);
            });
            assert!(visual.debug_bounds("pending-loading").is_some());
            assert!(visual.debug_bounds("empty-pending-empty").is_none());
            assert!(visual.debug_bounds("empty-empty-empty").is_some());
            assert!(visual.debug_bounds("empty-loading").is_none());
            assert!(visual.debug_bounds("refresh-loading").is_none());
            assert!(visual.debug_bounds("empty-refresh-empty").is_none());
            assert!(visual.debug_bounds("retained").is_some());
            assert!(visual.debug_bounds("layout-loading").is_none());
            let pending = visual.debug_bounds("pending-loading-0").unwrap();
            let retained = visual.debug_bounds("retained").unwrap();
            assert_eq!(pending.size, retained.size);
            assert_eq!(pending.size.width, px(width - 32.));
        }
    }
    visual.update(|window, _| window.remove_window());
}
