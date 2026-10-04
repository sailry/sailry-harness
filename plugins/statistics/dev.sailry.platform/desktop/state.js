// Query choices and pagination are presentation state; core combines canonical reports.
const DAY = 86400000;
const revoked = view => ['not_configured','not_found','permission_denied','revision_conflict'].includes(view?.error?.code);
export const nodeId = value => Array.isArray(value) ? value.map(byte=>byte.toString(16).padStart(2,'0')).join('') : value;
export function query(days,now = Date.now()) {
  const end_ms = (Math.floor(now/DAY)+1)*DAY;
  return {start_ms:end_ms-days*DAY,end_ms,dimension:'model',projects:[],worktrees:[],providers:[],models:[],before:null};
}
export class State {
  constructor(node) {
    this.node=nodeId(node);this.days=365;this.query=query(this.days);this.period='daily';
    this.generation=0;this.cursor='';this.page=1;this.cursors=[null];this.view=null;this.previous=null;
  }
  data() {return this.previous ?? this.view?.report ?? null;}
  ready() {return !this.previous && !!this.data() && this.view.connected && !this.view.refreshing && !this.view.error;}
  reset() {this.page=1;this.cursors=[null];this.query.before=null;this.previous=null;}
  select(field,value) {
    if (field === 'range') {this.days=Number(value);Object.assign(this.query,query(this.days),{projects:this.query.projects,providers:this.query.providers,models:this.query.models});}
    else if(field === 'project') {this.query.projects=value ? [value] : [];this.query.worktrees=[];this.query.providers=[];this.query.models=[];}
    else if(field === 'provider') {this.query.providers=value ? [value] : [];this.query.models=[];}
    else if(field === 'model') this.query.models=value ? [value] : [];
    else return false;
    this.reset();this.view=null;return true;
  }
  turn(page) {
    if(!this.ready() || page<1 || page===this.page || page>this.cursors.length+1)return false;
    const data=this.data();
    if(page>this.cursors.length) {
      if(page!==this.page+1 || !data.requests.has_more || !data.requests.items.length)return false;
      this.cursors.push(data.requests.items.at(-1).position);
    }
    this.previous=data;this.page=page;this.query.before=this.cursors[page-1];this.view=null;return true;
  }
  start() {this.cursor='';return ++this.generation;}
  accept(event,generation) {
    if(generation!==this.generation || event.all ||
      this.cursor && (event.cursor.length<this.cursor.length || event.cursor.length===this.cursor.length && event.cursor<this.cursor))return false;
    this.cursor=event.cursor;this.view=event.view;
    if(revoked(this.view))this.previous=null;
    if(this.view?.report && !this.view.refreshing)this.previous=null;
    return true;
  }
}
export function choices(state,inventory,resources,text) {
  const projects=new Map([['',text.usage_all_projects]]),providers=new Map([['',text.usage_all_providers]]),models=new Set();
  const available = new Set(resources.filter(key=>key.kind==='provider').map(key=>key.data));
  for(const project of inventory?.projects ?? []) projects.set(project.id,project.name);
  for(const provider of inventory?.providers ?? []) {
    if(!state.query.projects.length || available.has(provider.id))providers.set(provider.id,provider.name);
    if(!state.query.providers.length || state.query.providers.includes(provider.id))for(const model of provider.models)models.add(model);
  }
  for(const resource of resources) {
    if(resource.kind==='project' && !projects.has(resource.data))projects.set(resource.data,resource.data);
    if(resource.kind==='provider' && !providers.has(resource.data))providers.set(resource.data,resource.data);
    if(resource.kind==='model' && (!state.query.providers.length || state.query.providers.includes(resource.data.provider)))models.add(resource.data.model);
  }
  const entries=map=>[...map].map(([id,label])=>({id,label}));
  const keep=(map,value)=>{if(value&&!map.has(value))map.set(value,value);return entries(map);};
  return {project:keep(projects,state.query.projects[0]),provider:keep(providers,state.query.providers[0]),
    model:keep(new Map([['',text.usage_all_models],...[...models].sort().map(value=>[value,value])]),state.query.models[0])};
}
