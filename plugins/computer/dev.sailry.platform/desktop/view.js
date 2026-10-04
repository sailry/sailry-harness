// Preserve the permission page composition from Sailry ab5251de desktop/view.rs.
import {div} from 'gpui-kit';
import {Button, Icon} from 'gpui-component';
import {theme} from 'sailry';
import {SettingsGroup, IconButton} from 'sailry/ui';

export function render(view) {
  const text = view.text, colors = theme().colors, value = view.value();
  const rows = [
    ['screen_capture','computer_screen_capture','screen','screen-recording'],
    ['accessibility','computer_accessibility','accessibility','accessibility']
  ].map(([permission,key,selector,icon]) => {
    const status = view.status(permission), canRequest = view.canRequest(permission);
    const control = canRequest
      ? new Button(`computer-authorize-${selector}`).label(text.permission_request)
        .disabled(view.pending).on_click((_,cx) => view.read(permission,cx))
      : div().id(`computer-${selector}-${status}`).text_color(colors.muted_foreground).child(text[status]);
    return div().id(`settings-row-${key}`).h_flex().w_full().py_3().gap_4().text_sm()
      .child(div().flex_1().min_w_0().h_flex().gap_2().text_color(colors.group_box_foreground)
        .child(div().id(`settings-icon-${key}`).flex_shrink(0)
          .child(new Icon(`icons/reicon/${icon}.svg`).size_4()))
        .child(text[key]))
      .child(div().id(canRequest ? `computer-authorize-${selector}` : `computer-status-${selector}`)
        .h_flex().flex_shrink(0).min_w_0().max_w_full().justify_end().child(control));
  });
  let help;
  if (value?.platform && value.platform !== 'macos') help = 'computer_permissions_unsupported';
  else if (value && !value.local && (value.screen_capture !== true || value.accessibility !== true)) {
    help = 'computer_permissions_remote';
  }
  return div().id('computer-permissions').v_flex().gap_4()
    .child(SettingsGroup.new('computer_permissions',{title:text.computer_permissions,header_action:true})
      .child(IconButton.new('computer-permissions-refresh',{
        icon:'redo',label:text.computer_permissions_refresh,disabled:!view.connected || view.pending
      })).children(rows))
    .children(help ? [div().id(help).text_color(colors.muted_foreground).child(text[help])] : []);
}
