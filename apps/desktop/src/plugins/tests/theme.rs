use super::*;

#[gpui::test]
fn publishes_alpha_colors(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        std::fs::write(
            fixture
                .directory
                .path()
                .join("project/package/dev.sailry.platform/desktop/main.js"),
            r#"
import { div } from "gpui-kit";
import { theme } from "sailry";
export default class {
  render() {
    const value = theme();
    return div().child(`${value.is_dark}:${value.colors.muted}:${value.colors.foreground}`);
  }
}
"#,
        )
        .unwrap();
        fixture.install(0);
        let (panel, visual) = mount(&fixture, cx);
        open_package(visual, &panel, "project-summary");
        wait(visual, |cx| panel.read(cx).mounted.is_some());
        for mode in [ThemeMode::Light, ThemeMode::Dark, ThemeMode::Light] {
            visual.update(|window, cx| Theme::change(mode, Some(window), cx));
            let expected = visual.update(|_, cx| {
                let value = crate::plugins::theme::snapshot(cx);
                let colors = value.get("colors").unwrap();
                format!(
                    "{}:{}:{}",
                    value.get("is_dark").unwrap().as_bool().unwrap(),
                    colors.get("muted").unwrap().as_str().unwrap(),
                    colors.get("foreground").unwrap().as_str().unwrap()
                )
            });
            wait(visual, |cx| snapshot(&panel, cx).contains(&expected));
        }
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}
