import {div} from 'gpui-kit';
import {Button,Checkbox,HorizontalRadioGroup,Radio,VForm,Field} from 'gpui-component';
import {SelectField,SegmentedTabs} from 'sailry/ui';
import {TextField} from 'sailry/forms';
import {SecretField} from 'sailry/credentials';
import {setSharing} from './editor.js';

export function sharing(view,owner) {
  const {text}=view,scopes=['private','global','projects'],kind=owner.sharing?.scope??'private';
  const writable=()=>view.editor===owner&&!view.locked();
  const choices=div().v_flex().gap_2().w_full()
    .child(new HorizontalRadioGroup('db-sharing').gap_4().flex_wrap().w_full()
      .disabled(view.locked()).selected_index(scopes.indexOf(kind))
      .children(scopes.map(scope=>new Radio(`db-sharing-${scope}`).text_sm().label(text[`sharing_${scope}`])))
      .on_change((index,cx)=>{if(!writable())return;setSharing(owner,scopes[index]);cx.notify();}))
    .children(kind==='projects'?[div().id('db-sharing-projects').v_flex().max_h(192).overflow_y_scroll().gap_2().py_1()
      .children(view.projects.length?view.projects.map(project=>new Checkbox(`db-sharing-${project.id}`).text_sm()
        .label(project.name).checked(owner.projects.includes(project.id)).disabled(view.locked())
        .on_change((checked,cx)=>{
          if(!writable()||owner.sharing?.scope!=='projects')return;
          owner.projects=checked?[...new Set([...owner.projects,project.id])]:owner.projects.filter(id=>id!==project.id);
          setSharing(owner,'projects');cx.notify();
        })):[div().text_sm().child(text.sharing_empty)])]:[]);
  return new VForm().child(new Field().label(text.sharing_title).child(choices));
}

export function editor(view) {
  const {editor:owner,text}=view,locked=view.locked(),network=owner.engine!=='sqlite';
  const field=(index,label)=>new Field().label(text[label]).child(TextField.new(owner.inputs[index],{disabled:locked}));
  let form=new VForm().child(field(0,'settings_name'));
  form=form.child(!network||owner.method==='socket'?field(1,!network?'db_path':owner.engine==='postgres'?'db_socket_directory':'db_socket'):field(2,'db_host'));
  if(network) {
    if(owner.method!=='socket'||owner.engine==='postgres')form=form.child(field(3,'db_port'));
    form=form.child(field(4,'db_name')).child(field(5,'db_username'))
      .child(new Field().label(text.db_password).child(SecretField.new(owner.secret,{disabled:locked})));
    if(owner.method!=='socket')form=form.child(new Field().label(text.db_tls).child(SelectField.new(`db-tls-${view.dialog}`,{
      label:text.db_tls,selected:owner.tls,disabled:locked,placeholder:text.db_tls,
      items:['require','prefer','disable'].map(id=>({id,label:text[`db_tls_${id}`]})),
    })));
    if(owner.method==='ssh')form=form.child(new Field().label(text.db_ssh_connection).child(SelectField.new(`db-ssh-${view.dialog}`,{
      label:text.db_ssh_connection,placeholder:text.db_ssh_select,
      selected:owner.ssh,disabled:locked||!view.ssh.length,items:view.ssh.map(profile=>({id:profile.id,label:profile.name})),
    })));
  }
  const engine=SegmentedTabs.new(`db-engine-${view.dialog}`,{
    selected:owner.engine,disabled:locked,items:['sqlite','mysql','postgres'].map(id=>({id,label:text[`db_${id}`]})),
  });
  const transport=SegmentedTabs.new(`db-transport-${view.dialog}`,{
    selected:owner.method,disabled:locked,items:['tcp','socket','ssh'].map(id=>({id,label:text[`db_transport_${id}`]})),
  });
  return div().id('db-editor').v_flex().gap_4().w_full().child(engine).children(network?[transport]:[]).child(form)
    .child(sharing(view,owner)).child(new VForm().child(new Field().child(new Checkbox('db-read-only').text_sm()
      .label(text.db_read_only).checked(owner.read_only).disabled(locked).on_change((value,cx)=>{
        if(view.editor!==owner||view.locked())return;owner.read_only=value;cx.notify();
      }))));
}

export function editorFooter(view) {
  const {editor:owner,text}=view,locked=view.locked();
  return div().h_flex().justify_end().gap_2()
    .child(new Button('db-cancel-edit').label(text.settings_cancel).on_click((_,cx)=>{if(view.editor===owner)view.close(cx);}))
    .child(new Button('db-test-edit').label(text.db_check).disabled(locked).on_click((_,cx)=>{if(view.editor===owner&&!view.locked())view.save(true,cx);}))
    .child(new Button('db-save').primary().label(text.settings_save).disabled(locked).on_click((_,cx)=>{if(view.editor===owner&&!view.locked())view.save(false,cx);}));
}
