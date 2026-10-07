import {View,div} from 'gpui-kit';
import {Button} from 'gpui-component';
import {ResourceTree,nextTreeEvent} from 'sailry/ui';
import {Anchor,record} from 'sailry/test';

export default class DeepTree extends View {
  init(_props,cx) {
    this.current=null;
    this.revision=0;
    this.items=[{
      id:'leaf',label:'长文件名.rs',
      decoration:{badges_width:96,badges:[{text:'+237',tone:'success'},{text:'−164',tone:'danger'}],
        check:{id:'deep-check',label:'Stage file',checked:false}},
      menu:[
        {id:'open',label:'Open',value:{resource:{node:'fixture',worktree:'tree',entry:{path:'长目录/leaf.rs',revision:1}},selection:['leaf'],metadata:{source:'deep'}}},
        {separator:true},
        {id:'remove',label:'Remove',enabled:false}
      ]
    }];
    for(let depth=23;depth>=0;depth--) {
      this.items=[{id:`folder-${depth}`,label:`Folder ${depth}`,expanded:true,children:this.items}];
    }
    cx.spawn(async cx=>{
      while(true) {
        let event;
        try {event=await nextTreeEvent();} catch {return;}
        if(event.kind==='select')this.current=event.id;
        record(JSON.stringify(event));
        cx.notify();
      }
    });
  }
  render() {
    return div().v_flex().size_full().min_h_0()
      .child(Anchor.new('refresh').child(new Button('refresh').label(`Refresh ${this.revision}`)
        .on_click((_,cx)=>{this.revision++;cx.notify();})))
      .child(div().flex_1().min_h_0().child(ResourceTree.new('deep-tree',{
        items:JSON.stringify(this.items),selected:this.current?[this.current]:[],current:this.current,guides:true,indent:12
      })));
  }
}
