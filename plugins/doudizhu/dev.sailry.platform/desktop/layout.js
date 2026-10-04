import { div } from "gpui-kit";
import { Image, theme } from "sailry";
// Original Sailry artwork reused from the city-trader package.
const townPattern = "dev.sailry.platform/desktop/assets/town-pattern.png";
export const portrait = player => `dev.sailry.platform/desktop/assets/character-${String(player + 1).padStart(2, "0")}.png`;

// Transparent gutters let this artwork repeat without stretching or edge seams.
const TILE_SIZE = 480;

function background(width, height) {
  const columns = Math.ceil(width / TILE_SIZE), rows = Math.ceil(height / TILE_SIZE);
  return div().id("ddz-background").absolute().inset(0).overflow_hidden()
    .opacity(theme().is_dark ? 0.24 : 0.08)
    .children(Array.from({ length: columns * rows }, (_, index) =>
      div().absolute().left(index % columns * TILE_SIZE).top(Math.floor(index / columns) * TILE_SIZE)
        .w(TILE_SIZE).h(TILE_SIZE)
        .child(Image.new(`ddz-pattern-${index}`, { path: townPattern }))));
}

export function stage(content, minimumWidth = 0) {
  const viewport = window.viewport_size();
  return div().id("ddz-stage").relative().flex_1().min_w(0).min_h(0).overflow_hidden()
    .child(background(viewport.width, viewport.height))
    .child(div().id("ddz-scroll").absolute().inset(0).overflow_x_scroll().overflow_y_scroll()
      .child(div().w_full().min_w(minimumWidth + 32).min_h("100%")
        .v_flex().items_center().p_4()
        .child(content.flex_shrink(0).my_auto())));
}
