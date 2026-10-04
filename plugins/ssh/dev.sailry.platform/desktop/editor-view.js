import {div} from 'gpui-kit';
import {Button,Checkbox,HorizontalRadioGroup,Radio,VForm,Field} from 'gpui-component';
import {SegmentedTabs} from 'sailry/ui';
import {TextField} from 'sailry/forms';
import {SecretField,describeSecret} from 'sailry/credentials';
import {setSharing} from './editor.js';

function sharing(view,owner) {
  const {text}=view,scopes=['private','global','projects'],kind=owner.sharing?.scope??'private';
  const writable=()=>view.editor===owner&&!view.locked();
  const choices=div().v_flex().gap_2().w_full()
    .child(new HorizontalRadioGroup('ssh-sharing').gap_4().flex_wrap().w_full()
      .disabled(view.locked()).selected_index(scopes.indexOf(kind))
      .children(scopes.map(scope=>new Radio(`ssh-sharing-${scope}`).text_sm().label(text[`sharing_${scope}`])))
      .on_change((index,cx)=>{if(!writable())return;setSharing(owner,scopes[index]);cx.notify();}))
    .children(kind==='projects'?[div().id('ssh-sharing-projects').v_flex().max_h(192).overflow_y_scroll().gap_2().py_1()
      .children(view.projects.length?view.projects.map(project=>new Checkbox(`ssh-sharing-${project.id}`).text_sm()
        .label(project.name).checked(owner.projects.includes(project.id)).disabled(view.locked())
        .on_change((checked,cx)=>{
          if(!writable()||owner.sharing?.scope!=='projects')return;
          owner.projects=checked?[...new Set([...owner.projects,project.id])]:owner.projects.filter(id=>id!==project.id);
          setSharing(owner,'projects');cx.notify();
        })):[div().text_sm().child(text.sharing_empty)])]:[]);
  return new VForm().child(new Field().label(text.sharing_title).child(choices));
}

export function editor(view) {
  const {editor:owner,text}=view,locked=view.locked(),key=describeSecret(owner.key),picking=key.picking;
  const field=(index,label)=>new Field().label(text[label]).child(TextField.new(owner.inputs[index],{disabled:locked}));
  let form=new VForm().child(field(0,'settings_name')).child(field(1,'ssh_host')).child(field(2,'ssh_port')).child(field(3,'ssh_username'));
  if(owner.authentication==='password')form=form.child(new Field().label(text.ssh_password).child(SecretField.new(owner.password,{disabled:locked})));
  else {
    form=form.child(owner.authentication==='private_key'
      ?new Field().label(text.ssh_private_key).child(SecretField.new(owner.key,{disabled:locked}))
      :new Field().label(text.ssh_key_path).child(new Button('ssh-key-choose').w_full()
        .label(key.file??text.ssh_key_choose).disabled(locked||picking).on_click((_,cx)=>{
          if(view.editor===owner&&!view.locked()&&!describeSecret(owner.key).picking)view.chooseKey(cx);
        })));
    form=form.child(new Field().label(text.ssh_passphrase).child(SecretField.new(owner.passphrase,{disabled:locked})));
  }
  const authentication=SegmentedTabs.new(`ssh-authentication-${view.dialog}`,{
    selected:owner.authentication,disabled:locked||picking,
    items:['password','private_key','key_path'].map(id=>({id,label:text[id==='key_path'?'ssh_key_file':`ssh_${id}`]})),
  });
  return div().id('ssh-editor').v_flex().gap_4().w_full().child(authentication).child(form).child(sharing(view,owner))
    .children(owner.original.revision?[div().text_sm().child(text.ssh_keep_credential)]:[]);
}

export function editorFooter(view) {
  const {editor:owner,text}=view,locked=view.locked(),picking=describeSecret(owner.key).picking;
  return div().h_flex().justify_end().gap_2()
    .child(new Button('ssh-cancel-edit').label(text.settings_cancel).on_click((_,cx)=>{if(view.editor===owner)view.close(cx);}))
    .child(new Button('ssh-save').primary().label(text.settings_save).disabled(locked||picking).on_click((_,cx)=>{
      if(view.editor===owner&&!view.locked()&&!describeSecret(owner.key).picking)view.save(cx);
    }));
}
