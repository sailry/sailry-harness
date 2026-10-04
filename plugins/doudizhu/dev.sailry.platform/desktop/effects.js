import { div } from "gpui-kit";
import { Tag } from "gpui-component";
import { backs } from "./table.js";

export const tween = (element, duration = 200) => element
  .transition("opacity", { duration, easing: "ease-out" })
  .transition("top", { duration, easing: "ease-out" })
  .transition("left", { duration: 300, easing: "ease-out" });

export function effects(view, cx) {
  const motion = view.motion;
  if (!motion) return [];
  const layers = [];
  if (motion.dealing != null) {
    const step = motion.dealing;
    layers.push(div().id("ddz-dealing").absolute().inset_0().v_flex().items_center().justify_center()
      .child(div().relative().w(160).h(90).children([0, 1, 2].map(index =>
        tween(div().id(`ddz-deck-${index}`).absolute())
          .left(step < 3 ? 50 + (step % 2 ? index * 22 - 22 : index * 3) : (index - 1) * 90 + 50)
          .top(step === 3 ? (index === 1 ? 60 : -35) : index * 3)
          .opacity(step === 3 ? 0 : 1).child(backs(1, cx))))));
  }
  return layers;
}

export function fireworks(view) {
  const stage = view.motion?.fireworks;
  if (stage == null) return [];
  return [div().id("ddz-fireworks").absolute().inset_0().h_flex().items_center().justify_center()
    .child(div().relative().w(320).h(280).children(Array.from({ length: 24 }, (_, index) => {
      const burst = Math.floor(index / 8), angle = (index % 8) * Math.PI / 4;
      const radius = stage ? 50 + burst * 8 : 2;
      return tween(div().id(`ddz-spark-${index}`).absolute())
        .left(55 + burst * 102 + Math.cos(angle) * radius)
        .top(40 + (burst === 1 ? -25 : 15) + Math.sin(angle) * radius + (stage === 2 ? 20 : 0))
        .opacity(stage === 2 ? 0 : 1)
        .child(new Tag().variant("success").outline().bg("#00000000").border_0().p_0().text_sm().child("✦"));
    })))];
}
