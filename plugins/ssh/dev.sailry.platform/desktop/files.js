import {browseSshDirectory} from 'sailry/connections';
import {createText,setText,releaseText} from 'sailry/forms';
export const join=(directory,name)=>`${directory.replace(/\/$/,'')}/${name}`;
export const parent=path=>path==='/'?'/':path.includes('/')?path.slice(0,path.lastIndexOf('/'))||'/':'.';
export const name=path=>path.split('/').at(-1);
export class Files {
  constructor(profile,report=()=>{},text={}) {this.profile=profile;this.report=report;this.path='.';this.input=createText('.',{label:text.ssh_remote_path,placeholder:text.ssh_remote_path});this.entries=[];this.next=null;this.pending=false;this.error=null;this.selected=[];this.current=null;this.anchor=null;this.clipboard=null;this.revision=0;this.closed=false;}
  close(){this.closed=true;releaseText(this.input);}
  async load(path,cx,append=false) {
    if(this.closed||this.pending||!path.trim())return;this.pending=true;this.error=null;cx.notify();
    try {const value=await browseSshDirectory({profile:this.profile.id,expected_revision:this.profile.revision,path,after:append?this.next:null});
      if(this.closed)return;if(value.kind==='host_key_required'){this.error=value.changed?'ssh_key_changed':'ssh_key_unknown';this.report({},this.error);return;}
      if(value.kind!=='directory')throw new Error('invalid directory response');
      if(this.path!==value.path){this.selected=[];this.current=null;this.anchor=null;}
      this.path=value.path;setText(this.input,this.path);this.entries=append?[...this.entries,...value.entries]:value.entries;this.next=value.next;this.revision++;
    } catch(error){if(!this.closed){this.error='ssh_files_failed';this.report(error,this.error);}}finally{this.pending=false;cx.notify();}
  }
  entry(path){return this.entries.find(entry=>join(this.path,entry.name)===path);}
  select(path,event) {
    if(event.shift&&this.anchor){const paths=this.entries.map(entry=>join(this.path,entry.name)),start=paths.indexOf(this.anchor),end=paths.indexOf(path);if(start>=0&&end>=0)this.selected=[...new Set([...(event.additive?this.selected:[]),...paths.slice(Math.min(start,end),Math.max(start,end)+1)])];}
    else if(event.additive){this.selected=this.selected.includes(path)?this.selected.filter(value=>value!==path):[...this.selected,path];this.anchor=path;}
    else {this.selected=[path];this.anchor=path;}this.current=path;
  }
  items(menu){return this.entries.map(entry=>({id:join(this.path,entry.name),label:entry.name,icon:entry.kind==='directory'?'folder':'file',menu:menu(join(this.path,entry.name))}));}
}
