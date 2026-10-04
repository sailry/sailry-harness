// Keep supported media bindings on the shared Kit select field.
import {div} from 'gpui-kit';
import {theme} from 'sailry';
import {SelectField, SettingsGroup} from 'sailry/ui';

const roles = [
  ['media_image_understanding','vision'],
  ['media_image_generation','image'],
  ['media_video_generation','video']
];

export function render(view) {
  const colors = theme().colors;
  const rows = roles.map(([key,kind]) => {
    const items = view.items(key,kind);
    const control = SelectField.new(key,{label:view.text[key],placeholder:view.label(kind),disabled:view.busy(),
      selected:items.find(item=>item.checked)?.id ?? null,items:items.map(({id,label})=>({id,label}))});
    return div().id(`settings-row-${key}`).h_flex().w_full().py_3().gap_4().text_sm().flex_wrap()
      .child(div().flex_1().min_w(160).v_flex().gap_1()
        .child(div().id(`settings-label-${key}`).h_flex().gap_2()
          .text_color(colors.group_box_foreground).child(view.text[key])))
      .child(div().id(`settings-control-${key}`).h_flex().flex_shrink(0).min_w_0()
        .max_w_full().justify_end().w_64().ml_auto().child(control));
  });
  return div().id('media-settings').v_flex().gap_4()
    .child(SettingsGroup.new('media_roles',{title:view.text.media_roles}).children(rows));
}
