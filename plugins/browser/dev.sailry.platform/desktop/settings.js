// Local browser settings preserve Sailry c4bb60a2's desktop.rs/actions.rs.
import {View, div} from 'gpui-kit';
import {Button, Spinner} from 'gpui-component';
import {context, theme} from 'sailry';
import {readBrowserSettings, setBrowserPersistent, listBrowserProfiles, importBrowserProfile,
  SettingsGroup, Modal, modal_closed, IconButton, Toggle, nextControlEvent,toast} from 'sailry/ui';
import {messages, errorKey, imported} from './locales.js';

function row(id, label, description, control) {
  const colors = theme().colors;
  return div().id(`settings-row-${id}`).h_flex().w_full().py_3().gap_4().text_sm()
    .child(div().v_flex().flex_1().min_w_0().gap_1()
      .child(div().child(label))
      .child(div().text_color(colors.muted_foreground).child(description)))
    .child(div().h_flex().flex_shrink(0).min_w_0().max_w_full().justify_end().child(control));
}

export default class Settings extends View {
  init(_props, cx) {
    this.text = messages(JSON.parse(context()).locale);
    this.state = readBrowserSettings();
    this.busy = false; this.profiles = null;
    this.message = this.state.supported ? null : this.text.browser_import_unavailable;
    cx.spawn(async cx => {
      try { while (true) {
        if (await modal_closed() === 'browser-profiles-dialog') { this.profiles = null; cx.notify(); }
      } } catch (_) { /* Closing settings releases the dialog subscription. */ }
    });
    cx.spawn(async cx => {
      try { while (true) {
        const event = await nextControlEvent();
        if (event.id === 'browser-profiles-close') {
          this.profiles = null; cx.notify();
        } else if (event.id === 'browser-retain-data') this.persist(event.value,cx);
      } } catch (_) { /* Closing settings releases its controls. */ }
    });
  }
  scan(cx) {
    if (this.busy || !this.state.enabled) return;
    this.busy = true; this.message = null; cx.notify();
    cx.spawn(async cx => {
      try {
        const profiles = await listBrowserProfiles();
        if (profiles === null) return;
        if (profiles.length) this.profiles = profiles;
        else this.report(this.text.browser_chrome_missing);
      } catch (error) { this.report(this.text[errorKey(error,'browser_chrome_failed')]); }
      finally { this.busy = false; cx.notify(); }
    });
  }
  import(profile, cx) {
    if (this.busy || !this.state.enabled) return;
    const profiles=this.profiles;this.busy = true; this.message = null; cx.notify();
    cx.spawn(async cx => {
      try {
        const {count,skipped} = await importBrowserProfile(profile.id);
        if(this.profiles===profiles)this.profiles=null;
        this.report(count === 0 && skipped === 0 ? this.text.browser_chrome_empty : imported(this.text,count,skipped),'info');
      } catch (error) { this.report(this.text[errorKey(error,'browser_import_failed')]); }
      finally { this.busy = false; cx.notify(); }
    });
  }
  persist(value, cx) {
    if (this.busy || !this.state.enabled) return;
    try { setBrowserPersistent(value); this.state = readBrowserSettings(); }
    catch (error) { this.report(this.text[errorKey(error,'browser_store_unavailable')]); }
    cx.notify();
  }
  report(message,kind='error') { toast({id:'browser-settings',message,kind}); }
  render() {
    const {text,state} = this, disabled = this.busy || !state.enabled;
    return div().id('browser-settings-page').v_flex().gap_3()
      .children(state.supported ? [SettingsGroup.new('browser_data',{title:text.browser_data})
        .child(row('browser_chrome_import',text.browser_chrome_import,text.browser_chrome_description,
          new Button('browser-chrome-scan').secondary().size('small').label(text.browser_import).disabled(disabled)
            .on_click((_,cx) => this.scan(cx))))
        .child(row('browser_data_local',text.browser_data_local,text.browser_data_description,
          Toggle.new('browser-retain-data',{label:text.browser_data_local,checked:state.persistent,disabled})))] : [])
      .children(this.message ? [div().id('browser-settings-message').text_sm().child(this.message)] : [])
      .children(this.busy ? [new Spinner().size('small')] : [])
      .child(Modal.new('browser-profiles-dialog',{open:!!this.profiles})
        .children(this.profiles ? [div().id('browser-profile-picker').v_flex().w(Math.min(380,window.viewport_size().width - 88)).gap_3()
          .child(div().h_flex().items_center().gap_3()
            .child(div().flex_1().text_lg().font_semibold().child(text.browser_choose_profile))
            .child(IconButton.new('browser-profiles-close',{icon:'close',label:text.close})))
          .child(div().id('browser-profiles').v_flex().max_h_80().overflow_y_scroll().gap_1()
            .children(this.profiles.map((profile,index) => new Button(`browser-profile-${index}`).ghost().w_full().justify_start()
              .label(profile.name).on_click((_,cx) => this.import(profile,cx)))))] : []));
  }
}
