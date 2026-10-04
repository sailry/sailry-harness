import {View} from 'gpui-kit';
import {context} from 'sailry';
import {nextChange,readActivityCatalog,readActivityPreviews,nextActivityHosts} from 'sailry/sdk';
import {openActivityTarget,nextCardEvent,toast} from 'sailry/ui';
import {State} from './state.js';
import {messages} from './locales.js';
import {render} from './view.js';

export default class Workbench extends View {
  init(_props,cx) {
    this.text = messages(JSON.parse(context()).locale);
    // Kit releases registered native scroll state with this view's application generation.
    cx.spawn(async cx => {
      this.board = new State(readActivityCatalog,readActivityPreviews,() => cx.notify(),
        () => toast({id:'activity-read',kind:'error',message:this.text.activity_read_failed}));
      await this.board.refresh();
    });
    cx.spawn(async cx => {
      let seen = '';
      try {while (true) {
        const state = await nextChange(seen);seen = state.cursor;
        this.board.connected = state.connected;cx.notify();
        if (state.connected) await this.board.refresh();
      }} catch (_) { /* Ordinary view release ends observation. */ }
    });
    cx.spawn(async () => {
      let seen = {hosts:[],unread_terminals:[],visible:false};
      try {while (true) {seen = await nextActivityHosts(seen);this.board.hosts(seen);}}
      catch (_) { /* The shared controller keeps its own notification history. */ }
    });
    cx.spawn(async () => {
      try {while (true) {
        const id = await nextCardEvent(), target = this.targets.get(id);
        if (target) openActivityTarget(target.kind,target.id);
      }} catch (_) { /* Released cards cannot navigate replacement views. */ }
    });
  }
  render(cx) {return render(this,cx);}
}
