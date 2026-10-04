import {View,div} from 'gpui-kit';
import {context} from 'sailry';
import {publishContributions} from 'sailry/sdk';
import {readComposer,nextComposerChange} from 'sailry/ui';
import {messages} from './locales.js';
import {metrics} from './metrics.js';

export default class Composer extends View {
  init(_props,cx) {
    const text = messages(JSON.parse(context()).locale);
    let state = readComposer();
    publishContributions(metrics(state,text));
    cx.spawn(async () => {
      try {while(true) {
        state = await nextComposerChange(state.cursor);
        publishContributions(metrics(state,text));
      }} catch (_) { /* The captured composer owns the observation lifetime. */ }
    });
  }
  render() {return div();}
}
