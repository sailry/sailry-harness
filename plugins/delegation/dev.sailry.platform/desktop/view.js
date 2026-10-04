// Existing role records and forms composed with the same native Kit controls.
import {div} from 'gpui-kit';
import {Button,Field,Form} from 'gpui-component';
import {theme} from 'sailry';
import {TextField} from 'sailry/forms';
import {EmptyState,SettingsGroup,IconButton,Appearance,AppearancePicker,Menu,Modal} from 'sailry/ui';
import {efforts} from './draft.js';

function button(id,label,action,disabled = false,primary = false) {
  let control = new Button(id).label(label).disabled(disabled).on_click(action);
  if (primary) control = control.primary();
  return div().id(id).child(control);
}
function choice(owner,value,label,checked) {
  return {id:`${owner.token}:${value}`,label,enabled:true,checked};
}
function effortLabel(view,value) {
  if (value === null || value === 'default') return view.text.role_effort_inherit;
  if (typeof value === 'object') return value.budget === -1 ? view.text.effort_dynamic
    : view.text.effort_budget.replace('%{tokens}',String(value.budget));
  return value;
}
function empty(view) {
  return EmptyState.new('role-empty',{variant:'list',icon:'inbox',label:view.text.settings_empty});
}
function editor(view,owner) {
  const text = view.text, locked = !!owner.request, props = {readonly:locked};
  const source = Menu.new('role-source',{label:text[owner.fixed ? 'role_fixed' : 'role_inherit'],form:true,disabled:locked,
    items:[choice(owner,'inherit',text.role_inherit,!owner.fixed),choice(owner,'fixed',text.role_fixed,owner.fixed)]});
  const model = Menu.new('role-model',{label:owner.model?.model ?? text.media_unconfigured,form:true,disabled:locked,
    items:owner.models.map(model => choice(owner,model.id,`${model.provider} / ${model.model}`,
      model.id === `${owner.model?.provider}/${owner.model?.model}`))});
  const effort = Menu.new('role-effort',{label:effortLabel(view,owner.model?.effort ?? null),form:true,disabled:locked,
    items:efforts(owner).map(value => choice(owner,JSON.stringify(value),effortLabel(view,value),
      JSON.stringify(value) === JSON.stringify(owner.model?.effort === 'default' ? null : owner.model?.effort ?? null)))});
  const form = new Form()
    .child(new Field().label(text.role_id).child(div().id('role-field-role_id').child(TextField.new(owner.fields.key,props))))
    .child(new Field().label(text.settings_name).child(div().id('role-field-settings_name')
      .child(TextField.new(owner.fields.name,{...props,adornment:'prefix'})
        .child(AppearancePicker.new('role',{label:text.role_appearance,value:owner.appearance,disabled:locked})))))
    .child(new Field().label(text.role_model_source).child(source))
    .children(owner.fixed ? [new Field().label(text.provider_models).child(model)] : [])
    .child(new Field().label(text.provider_default_effort).child(effort))
    .child(new Field().label(text.role_max_turns).child(div().id('role-max-turns').child(TextField.new(owner.fields.turns,props))));
  const instructions = div().v_flex().gap_2()
    .child(div().h_flex().w_full().justify_between().gap_2().text_sm().font_medium()
      .child(text.role_instructions)
      .child(Menu.new('role-presets',{label:text.role_presets,small:true,disabled:locked,
        items:['review','research','implement'].map(key => choice(owner,key,text[`role_preset_${key}`],false))})))
    .child(new Form().child(new Field().label_indent(false)
      .child(div().id('role-instructions').child(TextField.new(owner.fields.instructions,props)))));
  return div().id('role-editor').v_flex().w(Math.min(600,window.viewport_size().width-88)).gap_4()
    .child(div().text_lg().font_semibold().child(text.role_editor))
    .child(div().id('role-editor-scroll').v_flex().min_h_0().max_h(Math.max(160,window.viewport_size().height-230))
      .overflow_y_scroll().p_1().gap_4().child(form).child(instructions))
    .child(div().h_flex().justify_end().gap_2()
      .child(button('role-cancel',text.settings_cancel,(_,cx) => view.close(cx)))
      .child(button('role-save',text[owner.request ? 'role_retry' : 'role_save'],(_,cx) => view.save(cx),
        owner.pending || !view.connected,true)));
}
function removal(view,owner) {
  const text = view.text;
  return div().id('role-removal').v_flex().w(Math.min(400,window.viewport_size().width-88)).gap_4()
    .child(div().text_lg().font_semibold().child(text.settings_delete))
    .child(div().text_sm().child(text.role_remove_live.replace('%{name}',owner.original.name)))
    .child(div().h_flex().justify_end().gap_2()
      .child(button('role-delete-cancel',text.settings_cancel,(_,cx) => view.close(cx)))
      .child(button('role-delete-confirm',text[owner.request ? 'role_retry' : 'settings_delete'],
        (_,cx) => view.save(cx),owner.pending || !view.connected,true)));
}
export function render(view) {
  const text = view.text, disabled = !view.ready || !view.connected;
  const rows = view.roles.map(role => div().id(`role-${role.id}`).w_full().text_sm().py_3()
    .child(div().h_flex().w_full().gap_3()
      .child(div().flex_1().min_w_0().h_flex().gap_2()
        .child(Appearance.new(`role-icon-${role.id}`,{value:role.appearance ?? {icon:'ai',color:'none'}}))
        .child(div().v_flex().min_w_0().gap_1().child(div().truncate().child(role.name))
          .child(div().text_sm().text_color(theme().colors.muted_foreground).truncate().child(role.key))))
      .child(div().h_flex().flex_shrink(0).gap_2()
        .child(IconButton.new(`role-edit-${role.id}`,{icon:'settings-2',label:text.settings_edit,disabled}))
        .child(IconButton.new(`role-delete-${role.id}`,{icon:'circle-x',label:text.settings_delete,disabled})))));
  let modal = Modal.new(view.dialog?.token ?? 'role-dialog',{open:!!view.dialog});
  if (view.dialog) modal = modal.child(view.dialog.kind === 'edit' ? editor(view,view.dialog) : removal(view,view.dialog));
  return div().id('role-settings').v_flex().gap_6()
    .child(SettingsGroup.new('roles_profiles',{title:text.roles_profiles,header_action:true})
      .child(button('role-add',text.settings_add,(_,cx) => view.edit(null,cx),disabled,true)).children(rows.length ? rows : [empty(view)]))
    .child(modal);
}
