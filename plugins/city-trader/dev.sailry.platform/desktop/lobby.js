import { div } from "gpui-kit";
import { Toggle, Spinner } from "gpui-component";
import { Image, theme } from "sailry";
import { building, character } from "./art.js";
import { CATALOG } from "./data.js";
import { button } from "./controls.js";
import { stage } from "./layout.js";

export function lobby(view, header) {
  const { text } = view, colors = theme().colors;
  const key = view.loading ? "loading" : view.available ? "ready" : view.error || "configure";
  const content = div().id("city-lobby").v_flex().items_center().gap_4().w_full().max_w(560)
    .child(div().h_flex().items_end().justify_center()
      .children([1, 11, 17].map((index, i) => div().w(i === 1 ? 130 : 110).h(i === 1 ? 130 : 110)
        .child(Image.new(`city-lobby-building-${index}`, { path: building(CATALOG[index]) })))))
    .child(div().text_lg().font_semibold().child(text.title))
    .child(div().text_sm().text_color(colors.muted_foreground).child(text.subtitle))
    .child(div().h_flex().gap_2().w_full().children(view.names.map((name, index) => view.enabled?.[index] === false ? null :
      new Toggle(`city-choose-seat-${index}`).outline().flex_1().h(72)
        .checked(view.editingCharacter === index)
        .on_change((_checked, cx) => { view.editingCharacter = index; cx.notify(); })
        .child(div().h_flex().items_center().gap_2()
          .child(div().w(40).h(40).child(Image.new(`city-lobby-avatar-${index}`, { path: character(view.characters[index], true) })))
          .child(div().text_sm().child(name)))).filter(Boolean)))
    .child(div().h_flex().gap_2().children(Array.from({ length: 6 }, (_, index) =>
      new Toggle(`city-character-${index}`).w(70).h(88).p_1()
        .checked(view.characters[view.editingCharacter] === index)
        .tooltip(text.character(index + 1))
        .on_change((_checked, cx) => view.chooseCharacter(index, cx))
        .child(div().w(64).h(80).child(Image.new(`city-character-art-${index}`, { path: character(index) }))))));
  if (key === "configure" && !view.loading) content.child(div().id(`city-status-${key}`).text_sm()
    .text_color(colors.muted_foreground).child(text[key]));
  if (view.loading) content.child(new Spinner());
  else if (view.available) content.child(div().id("city-status-ready")
    .child(button("city-start", text.start, cx => view.start(cx), false, true)));
  if (view.saved?.value) content.child(button("city-resume", text.resume, cx => view.resume(cx), !view.available));
  return div().id("city-root").size_full().v_flex().min_w(0).min_h(0).overflow_hidden()
    .child(header).child(stage(content));
}
