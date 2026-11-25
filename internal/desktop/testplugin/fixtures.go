package testplugin

const DesktopName = "desktop-fixture"
const StatusName = "status-fixture"

func Notification() Package {
	return Package{Name: "notification-fixture", Platform: `{"api_version":"v1","actions":["notifications.publish"],"display":{"label":"Fixture notifications","icon":"phosphor:bell"}}`}
}

func Desktop() Package {
	return Package{Name: DesktopName, Platform: `{
  "api_version":"v1","scope":"host","actions":["storage.read","models.generate","teams.read","activity.read"],
  "display":{"label":"Desktop fixture","locales":{"zh-CN":"桌面夹具"},"icon":"phosphor:puzzle-piece"},
  "settings_schema":"dev.sailry.platform/settings.json",
  "desktop":{"entry":"dev.sailry.platform/main.js","resources":["dev.sailry.platform/main.js"],
    "navigation":{"label":"Desktop fixture","locales":{"zh-CN":"桌面夹具"},"icon":"reicon:ui/box"},
    "navigation_options":{"pinned":true,"order":20}},
  "settings_page":{"entry":"dev.sailry.platform/main.js","navigation":{"label":"Fixture settings","locales":{"zh-CN":"夹具设置"},"icon":"reicon:ui/box"}}
}`, Files: map[string]string{
		"dev.sailry.platform/settings.json": `{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","additionalProperties":false,"properties":{"marker":{"type":"string","title":"Fixture marker","default":"desktop"}}}`,
		"dev.sailry.platform/main.js": `
import {View,div} from 'gpui-kit';
import {Button} from 'gpui-component';
import {context} from 'sailry';
import {readSettings,listKeys,readResourceCatalog,listModels,listTeams,readActivityCatalog} from 'sailry/sdk';
import {SettingsPage,SelectField,CardList,Modal,modal_closed} from 'sailry/ui';
export default class Fixture extends View {
  init(_props,cx) {
    const scope=JSON.parse(context());
    this.zh=scope.locale==='zh-CN'; this.ready=false; this.count=0; this.open=false; this.sessions=[];
    cx.spawn(async cx=>{
      const [settings,keys,resources,models,teams]=await Promise.all([readSettings(),listKeys(),readResourceCatalog(),listModels(),listTeams()]);
      if (!settings.ready || settings.values.marker!=='desktop' || !Array.isArray(keys.keys) ||
          !Array.isArray(resources.sources) || !Array.isArray(models.models) || !Array.isArray(teams)) {
        throw new Error('Invalid fixture data');
      }
      if (scope.surface==='workspace') {
        const activity=await readActivityCatalog();
        this.sessions=activity.sessions.map(session=>({title:session.activity.title,lane:activity.session_lanes[session.id]}));
      }
      this.ready=true; cx.notify();
    });
    cx.spawn(async cx=>{while(true){await modal_closed();this.open=false;cx.notify();}});
  }
  label(en,zh){return this.zh?zh:en;}
  render(){
    return SettingsPage.new('fixture-page',{title:this.label('Desktop fixture','桌面夹具'),
        description:this.ready?this.label('Fixture ready','已就绪'):this.label('Reading fixture','正在读取')})
      .child(div().h_flex().w_full().justify_between().gap(12)
        .child(div().w(192).child(SelectField.new('fixture-project',{
          label:this.label('Fixture project','夹具项目'),placeholder:this.label('All projects','全部项目'),selected:null,
          items:[{id:'local',label:this.label('Local project','本地项目')}]})))
        .child(div().h_flex().gap(8)
          .child(new Button('increment').label(this.label('Increment','递增')).on_click((_event,cx)=>{this.count++;cx.notify();}))
          .child(new Button('open-dialog').label(this.label('Open fixture dialog','打开夹具对话框')).on_click((_event,cx)=>{this.open=true;cx.notify();}))))
      .child(CardList.new('fixture-empty',{loaded:true,empty_icon:'calendar',empty_label:this.label('No fixture tasks','暂无夹具任务')}))
      .child(this.label('Fixture clicks: ','夹具点击：')+String(this.count))
      .children(this.sessions.map(session=>div().child(this.label('Activity: ','活动：')+session.title+' · '+session.lane)))
      .child(Modal.new('fixture-dialog',{open:this.open,form:true,title:this.label('Fixture dialog','夹具对话框')})
        .child(div().child(this.label('Fixture dialog body','夹具对话框内容'))));
  }
}`,
	}}
}

func Connectors() []Package {
	remote := Package{Name: "optional-fixture", Platform: `{
  "api_version":"v1","scope":"host","actions":[],
  "display":{"label":"Optional fixture","icon":"phosphor:plug"},
  "connectors":[{"id":"optional-fixture","display":{"label":"Optional fixture","icon":"phosphor:plug"},
    "accounts":"single","origins":["https://optional.example.invalid"],
    "auth":[{"kind":"secret","fields":["api_key"]}],
    "fields":[{"name":"api_key","title":"API key","secret":true,"optional":true,"locales":{"zh-CN":"API 密钥"}}],
    "inject":[{"server":"optional-fixture","header":"X-Fixture-Key"}]}]
}`, MCP: `{"$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json","mcpServers":{"optional-fixture":{"type":"streamable-http","url":"https://optional.example.invalid/mcp"}}}`}
	account := Package{Name: "account-fixture", Platform: `{
  "api_version":"v1","scope":"host","actions":[],
  "display":{"label":"Account fixture","icon":"phosphor:plug"},
  "connectors":[{"id":"account-fixture","display":{"label":"Account fixture","icon":"phosphor:plug"},
    "accounts":"single","origins":["https://account.example.invalid"],
    "auth":[{"kind":"secret","fields":["token"]},{"kind":"local_cli","command":"sailry-test-cli-not-installed",
      "status":{"argv":["status"],"account":"/account"},"token":{"argv":["token","{account}"],"parse":"line"},"timeout_ms":1000,"max_output":1024}],
    "fields":[{"name":"token","title":"Token","secret":true,"optional":false,"locales":{"zh-CN":"令牌"}}],
    "inject":[{"server":"account-fixture","header":"Authorization","prefix":"Bearer "}]}]
}`, MCP: `{"$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json","mcpServers":{"account-fixture":{"type":"streamable-http","url":"https://account.example.invalid/mcp"}}}`}
	return []Package{remote, account}
}

func Status() Package {
	return Package{Name: StatusName, Platform: `{
  "api_version":"v1","scope":"host","actions":["storage.read","storage.write","conversation.read","conversation.control"],
  "display":{"label":"Fixture status","icon":"phosphor:flag"},
  "storage":{"collections":[{"scope":"conversation","key":"state","schema":{"type":"object"}}]},
  "tools":[{"name":"fixture_status","description":"Read the fixture status","handler":{"name":"inspect","read_only":true,"parameters":{"type":"object","additionalProperties":false}}}],
  "desktop":{"ui_entry":"dev.sailry.platform/status.js","resources":["dev.sailry.platform/status.js"]},
  "ui":[{"id":"command","slot":"commands","kind":"button","command":{"name":"fixture","kind":"message"},"label":{"label":"Fixture status"},"icon":"reicon:ui/box","handler":"command"},
    {"id":"status","intent":"fixture","slot":"status","kind":"popover","label":{"label":"Fixture status"},"icon":"reicon:ui/box","handler":"status"}],
  "host":{"entry":"dev.sailry.platform/host.js","resources":["dev.sailry.platform/host.js"],
    "handlers":["inspect","control","command","badge"],"read_only":["inspect","badge"],
    "commands":[{"name":"fixture","handler":"command"}],"status":{"read":"badge","changes":["package","conversation"]}}
}`, Files: map[string]string{
		"dev.sailry.platform/host.js": `
import {context,getConversationValue,prepareTransaction,completeRequest,forgetRequest} from 'sailry/sdk';
export async function inspect(){const entry=await getConversationValue('state');return entry.present?entry.value:null;}
export async function command(){return {ui_intent:'fixture'};}
export async function badge(){
  if(!context().session)return [{id:'status',visible:false}];
  const state=await inspect();
  return [{id:'status',visible:!!state,enabled:true,label:{label:'Fixture status'},
    segments:state?[{label:{label:state.phase==='paused'?'Paused':'Running'},tone:'accent'},
      {label:{label:state.detail},tone:'muted'}]:[]}];
}
export async function control(args){
  const entry=await getConversationValue('state');
  const value=args.kind==='end'?null:{...entry.value,phase:args.kind==='pause'?'paused':'active',
    detail:args.kind==='pause'?'Paused by you':'Just started'};
  const data={key:'state',expected_revision:entry.revision};
  const operation=value===null?{kind:'conversation_remove',data}:{kind:'conversation_write',data:{...data,value}};
  const id=prepareTransaction([operation]);
  try {const result=await completeRequest(id);if(result.Err)throw new Error(result.Err.message);}
  finally {forgetRequest(id);}
  return value;
}`,
		"dev.sailry.platform/status.js": `
import {View,div} from 'gpui-kit';
import {Button} from 'gpui-component';
import {context,prepareRequest,completeRequest,forgetRequest,publishContributions,nextContributionEvent,closeContribution} from 'sailry/sdk';
export default class FixtureStatus extends View {
  init(_props,cx){
    this.open=false;this.value=null;this.busy=false;
    if(!context().session)return;
    publishContributions([{id:'command',enabled:true}]);
    cx.spawn(async cx=>{while(true){const event=await nextContributionEvent();
      if(event.id==='status'){
        this.open=event.kind==='invoke'||event.value===true;
        if(this.open){
          this.value=undefined;cx.notify();
          this.value=await this.request('read_plugin_callback','inspect',{});
        }
      }
      if(event.id==='command')publishContributions([{id:'command',enabled:true,reply_to:event.sequence}]);
      cx.notify();}});
  }
  async request(kind,handler,input){const id=prepareRequest({kind,data:{handler,input}});
    try{const result=await completeRequest(id);if(result.Err)throw new Error(result.Err.message);return result.Ok.data;}
    finally{forgetRequest(id);}}
  perform(kind,cx){if(this.busy)return;this.busy=true;cx.notify();cx.spawn(async cx=>{
    try{this.value=await this.request('call_plugin','control',{kind});if(kind==='end'){this.open=false;closeContribution('status');}}
    finally{this.busy=false;cx.notify();}});}
  render(){if(!this.open)return div();const paused=this.value?.phase==='paused';return div().flex().flex_col().gap(12).p(12)
    .child(this.value===undefined?'Reading fixture status':this.value?.text??'No fixture status')
    .child(paused?'Paused · '+this.value.detail:'')
    .children(this.value?[
      new Button('pause').label(paused?'Resume':'Pause').disabled(this.busy).on_click((_event,cx)=>this.perform(paused?'resume':'pause',cx)),
      new Button('end').label('End').disabled(this.busy).on_click((_event,cx)=>this.perform('end',cx))]:[]);}
}`,
	}}
}
