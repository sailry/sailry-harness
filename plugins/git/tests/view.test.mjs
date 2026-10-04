import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import {checkbox} from '../dev.sailry.platform/desktop/changes.js';

const source = (await readFile(new URL('../dev.sailry.platform/desktop/view.js',import.meta.url),'utf8'))
  .replace(/^import .*;\n/gm,'').replace(/^export /gm,'');
const {decorated} = vm.runInNewContext(`${source}\n({decorated})`, {checkbox});

class Element {
  constructor(kind,props = {}) { this.kind = kind; this.props = props; this.items = []; this.style = {}; }
  child(value) { this.items.push(value); return this; }
  children(values) { this.items.push(...values); return this; }
}
function element(kind,props) {
  const value = new Element(kind,props);
  const proxy = new Proxy(value,{get:(target,key) => key in target ? target[key] : (...args) => { target.style[key]=args; return proxy; }});
  return proxy;
}
const {history} = vm.runInNewContext(`${source}\n({history})`, {
  div:() => element('div'),
  theme:() => ({colors:{muted_foreground:'#999'}}),
  EmptyState:{new:(id,props) => element('empty',{id,...props})},
  SelectableRow:{new:(id,props) => element('row',{id,...props})},
  Icon:class { constructor(path) { return element('icon',{path}); } },
  Button:class { constructor() { throw Error('history rows must use the selectable Kit adapter'); } },
});
function descendants(element) {
  return typeof element === 'object' && element !== null ? [element,...element.items.flatMap(descendants)] : [];
}

test('main workspace starts its changes panel at 320 pixels', async () => {
  const main = (await readFile(new URL('../dev.sailry.platform/desktop/main.js',import.meta.url),'utf8'))
    .replace(/^import[\s\S]*?;\n/gm,'').replace('export default class','class');
  const Git = vm.runInNewContext(`${main}\nGit`,{
    View:class {},div:() => element('div'),
    Workspace:{new:(id,props) => element('workspace',{id,...props})},
    ActionScope:{new:(id,props) => element('actions',{id,...props})},
    views:{header:() => element('header'),content:() => element('document'),
      details:() => element('changes'),dialogs:() => element('dialogs')},
  });
  const view = new Git();
  Object.assign(view,{mode:'main',repo:{selected:null}});
  const workspace = descendants(view.render()).find(node => node.kind === 'workspace');
  assert.equal(workspace.props.default_details_width,320);
  assert.equal(workspace.props.min_details_width,260);
  assert.equal(workspace.props.navigation,'none');
});

test('tree leaves encode an empty child array for the native descriptor', () => {
  const entry = {path:'src/main.rs',conflicted:false,untracked:false,
    staged:null,unstaged:'modified',diff:{additions:1,deletions:0}};
  const leaf = {id:'tracked/src/main.rs',label:'main.rs',entry};
  const view = {expanded:new Map(),nodes:new Map(),text:{git_stage_file:'Stage file'},
    menus:{files:() => []},repo:{navigation:{hierarchical:false},writable:() => true}};
  const value = decorated(view,{id:'tracked',label:'Tracked',entries:[entry],children:[leaf]});
  assert.ok(Array.isArray(value.children));
  assert.ok(Array.isArray(value.children[0].children));
  assert.equal(value.children[0].children.length,0);
  assert.equal(value.children[0].decoration.selector,'git-tracked-src/main.rs');
});

test('deleted counts and file glyphs use the semantic danger tone',()=>{
  const view={expanded:new Map(),nodes:new Map(),text:{git_stage_file:'Stage file'},
    menus:{files:()=>[]},repo:{navigation:{hierarchical:false},writable:()=>true}};
  for(const kind of ['modified','deleted']) {
    const entry={path:'notes.txt',conflicted:false,untracked:false,staged:null,unstaged:kind,
      diff:{additions:3,deletions:5}};
    const leaf=decorated(view,{id:'tracked/notes.txt',label:'notes.txt',entry});
    assert.deepEqual(JSON.parse(JSON.stringify(leaf.decoration.badges)),[
      {text:'+3',tone:'success'},{text:'−5',tone:'danger'},
    ]);
    if(kind==='deleted')assert.equal(leaf.decoration.icon_tone,'danger');
  }
  const {stats}=vm.runInNewContext(`${source}\n({stats})`,{div:()=>element('div')});
  const colors={success:'#0f0',destructive:'#f00'};
  const values=stats(3,5,colors);
  assert.deepEqual(Array.from(values,value=>value.style.text_color[0]),[colors.success,colors.destructive]);
});

test('history rows preserve full identities and controlled selection', () => {
  const initial = 'a'.repeat(40), latest = 'b'.repeat(40);
  const view = {repo:{selected:`commit:${initial}`,history:{offset:0,next:null,entries:[
    {id:latest,message:'Latest\nBody',author:'Fixture',references:['heads/main']},
    {id:initial,message:'Initial',author:'Fixture',references:[]},
  ]}}};
  const tree = history(view), rows = descendants(tree).filter(element => element.kind === 'row');
  assert.deepEqual(rows.map(row => ({...row.props})),[
    {id:`git-commit-${latest}`,selected:false},
    {id:`git-commit-${initial}`,selected:true},
  ]);
  assert.ok(descendants(rows[0]).some(element => element.items.includes('Latest')));
  for(const [index,values] of [[0,[latest.slice(0,8),'Fixture','main']],[1,[initial.slice(0,8),'Fixture']]]) {
    const metadata=descendants(rows[index]).find(node=>node.style.id?.[0]===`git-history-metadata-${view.repo.history.entries[index].id}`);
    assert.deepEqual(descendants(metadata).flatMap(node=>node.items.filter(value=>typeof value==='string')),values);
    assert.deepEqual(metadata.style.text_xs,[]);
    assert.deepEqual(metadata.style.text_color,['#999']);
    assert.deepEqual(metadata.style.gap_2,[]);
    assert.deepEqual(rows[index].items[1].style.gap_1,[]);
    const icons=descendants(metadata).filter(node=>node.kind==='icon');
    assert.deepEqual(icons.map(node=>node.props.path),['reicon:it/hashtag','reicon:users/user',...(index===0?['reicon:newicons/hierarchy2']:[])]);
    for(const icon of icons) {
      assert.deepEqual(icon.style.size,['xsmall']);
      assert.deepEqual(icon.style.color,['#999']);
    }
  }
  assert.ok(descendants(rows[1]).some(element => element.items.includes('Initial')));
});

test('history metadata selects one exact local branch without extra refs', () => {
  const id='c'.repeat(40),entry={id,message:'Add session test fixtures',author:'duxweb',references:[]};
  const cases=[
    {current:'master',references:['heads/feature','heads/master','heads/sailry/task-17','remotes/origin/master','tags/v1','stash'],branch:'master'},
    {current:'missing',references:['heads/master','heads/sailry/task-17'],branch:'master'},
    {current:null,references:['heads/sailry/task-17'],branch:'sailry/task-17'},
    {current:'master',references:['remotes/origin/master','tags/v1','stash'],branch:null},
    {current:'master',references:[],branch:null},
  ];
  for(const scenario of cases) {
    const view={repo:{status:{branch:scenario.current},history:{offset:0,next:null,entries:[{...entry,references:scenario.references}]}}};
    const row=descendants(history(view)).find(node=>node.kind==='row');
    const metadata=descendants(row).find(node=>node.style.id?.[0]===`git-history-metadata-${id}`);
    const values=descendants(metadata).flatMap(node=>node.items.filter(value=>typeof value==='string'));
    assert.deepEqual(values,[id.slice(0,8),'duxweb',...(scenario.branch?[scenario.branch]:[])]);
    assert.equal(values.some(value=>value.includes(' · ')),false);
    assert.equal(descendants(metadata).filter(node=>node.kind==='icon').length,scenario.branch?3:2);
    assert.equal(row.props.id,`git-commit-${id}`);
    assert.ok(descendants(row).some(node=>node.items.includes(entry.message)));
  }
});

test('empty lists fill their viewport without cards', () => {
  const {changes,combined} = vm.runInNewContext(`${source}\n({changes,combined})`,{
    div:() => element('div'),theme:() => ({colors:{}}),
    EmptyState:{new:(id,props) => element('empty',{id,...props})},
  });
  const view = {text:{git_no_changes:'No changes',git_history_empty:'No commits'},
    repo:{status:{kind:'ready',entries:[]},history:{entries:[]}}};
  const changeList=changes(view),historyList=history(view);
  assert.deepEqual(changeList.style.flex_1,[]);
  assert.deepEqual(historyList.style.flex_1,[]);
  assert.deepEqual({...changeList.items[0].props},{id:'git_no_changes',variant:'list',fill_height:true,icon:'network',label:'No changes'});
  assert.deepEqual({...historyList.items[0].props},{id:'git_history_empty',variant:'list',fill_height:true,icon:'calendar',label:'No commits'});
  const document = combined(view,{request:{kind:'changes'},files:[]});
  assert.equal(descendants(document).some(node => node.kind === 'empty'),false);
  assert.ok(descendants(document).some(node => node.items.includes('No changes')));
});

test('uninitialized main content has a full-width action while the sidebar stays compact', () => {
  const {empty} = vm.runInNewContext(`${source}\n({empty})`,{
    div:() => element('div'),theme:() => ({colors:{}}),
    EmptyState:{new:(id,props) => element('empty',{id,...props})},
    IconButton:{new:(id,props) => element('button',{id,...props})},
  });
  const view = {text:{git_directory_title:'Not initialized',git_initialize:'Initialize Git'},
    repo:{status:{kind:'directory'},canInitialize:()=>true}};
  const main=empty(view),sidebar=empty(view,true);
  const action=descendants(main).find(node=>node.kind==='button');
  assert.equal(action.props.id,'git-initialize');assert.equal(action.props.full_width,true);
  assert.equal(action.props.disabled,false);
  assert.equal(main.props.variant,undefined);
  assert.equal(main.props.id,'git_directory_title');assert.equal(sidebar.props.id,'git_directory_title-sidebar');
  assert.equal(sidebar.props.variant,'list');assert.equal(sidebar.props.fill_height,true);
  assert.equal(descendants(sidebar).some(node=>node.kind==='button'),false);
});

test('main tabs belong to the Shell while embedded tabs remain in the panel', () => {
  const {header} = vm.runInNewContext(`${source}\n({header})`,{
    div:() => element('div'),Header:{new:(id,props) => element('metadata',{id,...props})},
    PanelHeader:{new:id => element('header',{id})},
    NavigationTabs:{new:(id,props) => element('tabs',{id,...props})},
  });
  const view = {mode:'main',text:{git:'Git',close:'Close'},
    repo:{selected:'file:notes.txt',tabs:['file:notes.txt'],documents:new Map([
      ['file:notes.txt',{request:{kind:'file',path:'notes.txt'}}],
    ])}};
  const main = header(view);
  assert.equal(main.kind,'metadata');
  const metadata = JSON.parse(main.props.content);
  assert.equal(metadata.title,'Git');
  assert.equal(metadata.tabs.id,'git-tabs');
  assert.deepEqual(metadata.tabs.items,[{id:'file:notes.txt',label:'notes.txt',closable:true,close_label:'Close'}]);
  view.mode = 'embedded';
  assert.equal(header(view).kind,'header');
  view.mode = 'main'; view.repo.tabs = [];
  assert.equal(JSON.parse(header(view).props.content).title,'Git');
});

test('unopened diffs omit the document status bar', () => {
  const {content} = vm.runInNewContext(`${source}\n({content})`,{
    div:() => element('div'),theme:() => ({colors:{}}),
    StatusBar:class {constructor(){return element('status');}},
    EmptyState:{new:(id,props) => element('empty',{id,...props})},
    NativeContextMenu:{new:(id,props) => element('menu',{id,...props})},
  });
  const tree=content({text:{git_diff_empty:'Select changes'},repo:{documents:new Map(),selected:null,status:{kind:'ready'}}});
  const nodes=descendants(tree);
  assert.equal(nodes.some(node=>node.kind==='status'),false);
  assert.equal(nodes.some(node=>node.style.id?.[0]==='document-path'),false);
  assert.ok(nodes.some(node=>node.kind==='empty'&&node.props.id==='git_diff_empty'));
  assert.equal(nodes.find(node=>node.kind==='empty').props.variant,undefined);
  assert.equal(nodes.some(node=>node.items.includes('+0 -0')),false);
  assert.equal(nodes.some(node=>node.kind==='menu'),false);
});

test('output documents omit diff status and path actions', () => {
  const {content} = vm.runInNewContext(`${source}\n({content})`,{
    div:() => element('div'),theme:() => ({colors:{}}),
    StatusBar:class {constructor(){return element('status');}},
    SourceSurface:{new:(id,props) => element('source',{id,...props})},
    NativeContextMenu:{new:(id,props) => element('menu',{id,...props})},
  });
  const document={id:'output:',request:{kind:'output'},text:'Git output'};
  const nodes=descendants(content({text:{git_output:'Output'},repo:{documents:new Map([[document.id,document]]),selected:document.id}}));
  assert.ok(nodes.some(node=>node.kind==='source'&&node.props.text===document.text));
  assert.equal(nodes.some(node=>node.kind==='status'),false);
  assert.equal(nodes.some(node=>node.style.id?.[0]==='document-path'),false);
  assert.equal(nodes.some(node=>node.kind==='menu'),false);
});

test('single diffs grow inside the document flex viewport', () => {
  const {content} = vm.runInNewContext(`${source}\n({content})`,{
    div:() => element('div'),theme:() => ({colors:{success:'success',destructive:'destructive'}}),
    StatusBar:class {constructor(){return element('status');}},
    DiffSurface:{new:(id,props) => element('diff',{id,...props})},
    NativeContextMenu:{new:(id,props) => element('menu',{id,...props})},
  });
  const document={id:'notes.txt',request:{kind:'file',path:'notes.txt'},
    files:[{path:'notes.txt',text:'@@ -1 +1 @@\n-before\n+after',additions:1,deletions:1}]};
  const tree=content({text:{},repo:{documents:new Map([[document.id,document]]),selected:document.id}});
  const bar=descendants(tree).find(node=>node.kind==='status');
  assert.ok(bar);assert.deepEqual(bar.style.h_8,[]);
  const added=descendants(bar).find(node=>node.items.includes('+1'));
  const removed=descendants(bar).find(node=>node.items.includes('−1'));
  assert.ok(added);assert.ok(removed);
  assert.deepEqual(added.style.text_color,['success']);
  assert.deepEqual(removed.style.text_color,['destructive']);
  const editor=descendants(tree).find(node=>node.style.id?.[0]==='document-editor');
  assert.deepEqual(editor.style.v_flex,[]);
  assert.deepEqual(editor.style.flex_1,[]);
  assert.deepEqual(editor.items[0].style.flex_1,[]);
  const diff=descendants(editor).find(node=>node.kind==='diff');
  assert.equal(diff.props.id,'git-diff-notes.txt');
  assert.equal(diff.props.text,document.files[0].text);
});

function diffViews() {
  return vm.runInNewContext(`${source}\n({content,combined})`,{
    div:() => element('div'),theme:() => ({colors:{muted_foreground:'muted'}}),
    StatusBar:class {constructor(){return element('status');}},
    DiffSurface:{new:(id,props)=>element('diff',{id,...props})},
    Collapse:{new:(id,props)=>element('collapse',{id,...props})},
    CollapseSlot:{new:(id,props)=>element('slot',{id,...props})},
    IconButton:{new:(id,props)=>element('button',{id,...props})},
    NativeContextMenu:{new:(id,props)=>element('menu',{id,...props})},
  });
}

test('binary documents show only the localized unavailable message', () => {
  const {content}=diffViews();
  const patch='diff --git a/image.png b/image.png\nnew file mode 100644\nindex 0000000..1234567\nBinary files /dev/null and b/image.png differ';
  const document={id:'image.png',request:{kind:'file',path:'image.png'},
    files:[{path:'image.png',text:patch,binary:true,truncated:true,additions:0,deletions:0}]};
  const view={text:{git_diff_binary:'Cannot display binary diff'},repo:{documents:new Map([[document.id,document]]),selected:document.id}};
  const nodes=descendants(content(view));
  assert.equal(nodes.some(node=>node.kind==='diff'),false);
  assert.equal(nodes.some(node=>node.items.includes(patch)),false);
  const notice=nodes.find(node=>node.items.includes(view.text.git_diff_binary));
  assert.ok(notice);assert.deepEqual(notice.style.text_sm,[]);assert.equal(notice.style.text_xs,undefined);
});

test('combined diffs preserve binary entries and text through explicit host slots', () => {
  const {combined}=diffViews();
  const files=[{path:'image.png',text:'Binary files a/image.png and b/image.png differ',binary:true,truncated:true,additions:0,deletions:0},
    {path:'large.txt',text:'@@ -1 +1 @@\n-before\n+after',binary:false,truncated:true,additions:1,deletions:1}];
  const document={id:'changes:',request:{kind:'changes'},files};
  const view={text:{git_diff_binary:'Cannot display binary diff',git_diff_partial:'Partial diff',turn_changes_undo:'Undo'},
    repo:{status:{entries:files.map(file=>({path:file.path,conflicted:false}))},writable:()=>true}};
  const nodes=descendants(combined(view,document)),panels=nodes.filter(node=>node.kind==='collapse');
  assert.equal(panels.length,2);
  for(let index=0;index<panels.length;index++) {
    assert.equal(panels[index].props.id,`diff-file-changes:/${files[index].path}`);
    assert.equal(panels[index].props.label,files[index].path);assert.equal(panels[index].props.default_open,true);
    assert.deepEqual(panels[index].items.map(slot=>slot.props.variant),['header','actions','content']);
  }
  const surfaces=nodes.filter(node=>node.kind==='diff');assert.equal(surfaces.length,1);
  assert.equal(surfaces[0].props.path,'large.txt');assert.equal(surfaces[0].props.text,files[1].text);
  assert.equal(nodes.filter(node=>node.items.includes(view.text.git_diff_binary)).length,1);
  assert.equal(nodes.filter(node=>node.items.includes(view.text.git_diff_partial)).length,1);
  assert.deepEqual(nodes.filter(node=>node.kind==='button').map(node=>node.props.id),['git-diff-undo-image.png','git-diff-undo-large.txt']);
});

test('branch selectors reuse native icon, label, and dropdown slots', () => {
  const {details} = vm.runInNewContext(`${source}\n({details})`,{
    div:() => element('div'),theme:() => ({colors:{}}),
    EmptyState:{new:(id,props)=>element('empty',{id,...props})},
    PanelHeader:{new:(id,props)=>element('header',{id,...props})},
    IconButton:{new:(id,props)=>element('button',{id,...props})},
    Menu:{new:id=>element('menu',{id})},SegmentedTabs:{new:id=>element('tabs',{id})},
  });
  for(const branch of ['main',null]) {
    const tree=details({text:{git_branches:'Branches'},repo:{view:'history',status:{branch},branches:{},history:{offset:0,next:null,entries:[]}},menus:{root:()=>[]}});
    const header=descendants(tree).find(node=>node.props.id==='git-changes-header');
    assert.equal(header.props.gap,0);
    const button=descendants(tree).find(node=>node.props.id==='git-branch-menu');
    assert.equal(button.props.icon,'network');
    assert.equal(button.props.label,branch??'Branches');
    assert.equal(button.props.show_label,true);
    assert.equal(button.props.dropdown_caret,true);
    assert.equal(button.props.disabled,false);
  }
});
