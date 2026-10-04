import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

const source = (await readFile(new URL('../dev.sailry.platform/desktop/explorer-view.js',import.meta.url),'utf8'))
  .replace(/^import .*;\n/gm,'').replace(/^export /gm,'');

function element(kind,props = {}) {
  const node = {kind,props,items:[]};
  const api = new Proxy(node,{get(target,key) {
    if (key in target) return target[key];
    return (...args) => {
      if (key === 'child') target.items.push(args[0]);
      else if (key === 'children') target.items.push(...args[0]);
      else target.props[key] = args.length === 1 ? args[0] : args;
      return api;
    };
  }});
  return api;
}

function all(node) {
  return node && typeof node === 'object' ? [node,...node.items.flatMap(all)] : [node];
}

test('main workspace starts its explorer at 320 pixels', async () => {
  const main = (await readFile(new URL('../dev.sailry.platform/desktop/main.js',import.meta.url),'utf8'))
    .replace(/^import[\s\S]*?;\n/gm,'').replace('export default class','class');
  const Files = vm.runInNewContext(`${main}\nFiles`,{
    View:class {},div:() => element('div'),
    Workspace:{new:(id,props) => element('workspace',{id,...props})},
    ActionScope:{new:(id,props) => element('actions',{id,...props})},
    Modal:{new:(id,props) => element('modal',{id,...props})},
    documentView:{header:() => element('header'),content:() => element('document')},
    explorerView:() => element('explorer'),
  });
  const view = new Files();
  Object.assign(view,{mode:'main',text:{file_tree:'File tree'},documents:{current:() => null},
    transfers:{value:() => null}});
  const workspace = all(view.render()).find(node => node?.kind === 'workspace');
  assert.equal(workspace.props.default_details_width,320);
  assert.equal(workspace.props.min_details_width,260);
  assert.equal(workspace.props.navigation,'none');
});

test('search results eagerly register native button actions with captured line targets', async () => {
  const search = vm.runInNewContext(`${source}\nsearch`,{
    div:() => element('div'),theme:() => ({colors:{}}),
    Button:function(id) {return element('button',{id});},
    TextField:{new:id => element('input',{id})},IconButton:{new:id => element('icon',{id})},
    Toggle:{new:(id,props) => element('toggle',{id,...props})},
  });
  const targets = [],cx = {};
  const view = {text:{},explorer:{connected:true},
    search:{query:'query',filter:'filter',case_sensitive:true,regex:false,result:{matches:[
      {path:'日本語/first.txt',line_number:7,line:'match'},
      {path:'second.txt',line_number:12,line:'other'},
    ]}},
    run:(action,eventCx) => {assert.equal(eventCx,cx); return action(cx);},
    open:async (...args) => targets.push(args),
  };
  const nodes = all(search(view));
  const sensitive = nodes.find(node => node?.props?.id === 'file-search-case');
  const regex = nodes.find(node => node?.props?.id === 'file-search-regex');
  assert.equal(sensitive.kind,'toggle');
  assert.equal(sensitive.props.variant,'button');
  assert.equal(sensitive.props.text,'Aa');
  assert.equal(sensitive.props.checked,true);
  assert.equal(regex.kind,'toggle');
  assert.equal(regex.props.text,'.*');
  assert.equal(regex.props.checked,false);
  const first = nodes.find(node => node?.props?.id === 'file-search-result-0');
  assert.equal(first.kind,'button');
  assert.equal(typeof first.props.on_click,'function');
  view.search.result = {matches:[]};
  await first.props.on_click(null,cx);
  assert.deepEqual(targets,[['日本語/first.txt',7,cx]]);
});

test('empty directories fill the sidebar without cards', () => {
  const render = vm.runInNewContext(`${source}\nrender`,{
    div:() => element('div'),theme:() => ({colors:{}}),
    PanelHeader:{new:id => element('header',{id})},
    IconButton:{new:id => element('icon',{id})},Menu:{new:id => element('menu',{id})},
    NativeContextMenu:{new:id => element('context',{id})},
    EmptyState:{new:(id,props) => element('empty',{id,...props})},
    ResourceTree:{new:() => assert.fail('an empty directory must not mount a tree')},
  });
  const view = {text:{files_directory_empty:'Empty folder'},search:null,
    explorer:{pages:new Map([['',{loaded:true,entries:[]}]]),connected:true,directory:() => ''},
    actions:{create:() => [],tree:() => []},
  };
  const empty = all(render(view)).find(node => node?.kind === 'empty');
  assert.deepEqual({...empty.props},{id:'files_directory_empty',variant:'list',fill_height:true,icon:'folder',label:'Empty folder'});
  view.explorer.pages.set('',{loaded:false,entries:[]});
  const loading = all(render(view)).find(node => node?.kind === 'empty');
  assert.equal(loading.props.id,'files_loading');
  assert.equal(loading.props.variant,'list');
  assert.equal(loading.props.fill_height,true);
});

test('empty search fills its viewport', () => {
  const search = vm.runInNewContext(`${source}\nsearch`,{
    div:() => element('div'),theme:() => ({colors:{}}),
    TextField:{new:id => element('input',{id})},IconButton:{new:id => element('icon',{id})},
    Toggle:{new:(id,props) => element('toggle',{id,...props})},
    EmptyState:{new:(id,props) => element('empty',{id,...props})},
  });
  const view = {text:{file_search_empty:'No matches',file_search_running:'Searching',file_search_partial:'Partial results'},
    explorer:{connected:true},search:{query:'retained query',filter:'retained filter',running:false,
      status:'file_search_empty',result:{matches:[]}}};
  let nodes = all(search(view)),empty = nodes.find(node => node?.kind === 'empty');
  assert.deepEqual({...empty.props},{id:'file_search_empty',variant:'list',fill_height:true,icon:'search',label:'No matches'});
  assert.equal(nodes.filter(node => node?.items?.includes('No matches')).length,0);
  for (const [status,running,label] of [['file_search_running',true,'Searching'],['file_search_partial',false,'Partial results']]) {
    Object.assign(view.search,{status,running});
    nodes = all(search(view));
    assert.equal(nodes.some(node => node?.kind === 'empty'),false);
    assert.ok(nodes.some(node => node?.items?.includes(label)));
    assert.equal(view.search.query,'retained query');
    assert.equal(view.search.filter,'retained filter');
  }
});

test('main document tabs publish into the Shell header and embedded tabs stay local', async () => {
  const documentSource = (await readFile(new URL('../dev.sailry.platform/desktop/document-view.js',import.meta.url),'utf8'))
    .replace(/^import .*;\n/gm,'').replace(/^export /gm,'');
  const header = vm.runInNewContext(`${documentSource}\nheader`,{
    div:() => element('div'),Header:{new:(id,props) => element('metadata',{id,...props})},
    PanelHeader:{new:id => element('header',{id})},
    NavigationTabs:{new:(id,props) => element('tabs',{id,...props})},
  });
  const view = {mode:'main',text:{files:'Files',close:'Close'},
    documents:{selected:'draft',items:() => [{id:'draft',path:'src/main.rs',dirty:true}]} };
  const main = header(view);
  assert.equal(main.kind,'metadata');
  const metadata = JSON.parse(main.props.content);
  assert.equal(metadata.title,'Files');
  assert.equal(metadata.tabs.id,'file-tabs');
  assert.equal(metadata.tabs.selected,'draft');
  assert.deepEqual(metadata.tabs.items,[{id:'draft',label:'main.rs',dirty:true,closable:true,close_label:'Close src/main.rs'}]);
  view.mode = 'embedded';
  const embedded = header(view);
  assert.equal(embedded.kind,'header');
  assert.equal(all(embedded).filter(node => node?.kind === 'tabs').length,1);
  view.mode = 'main'; view.documents.items = () => [];
  assert.equal(JSON.parse(header(view).props.content).title,'Files');
});

test('empty document content uses the full pane without a blank footer', async () => {
  const documentSource = (await readFile(new URL('../dev.sailry.platform/desktop/document-view.js',import.meta.url),'utf8'))
    .replace(/^import .*;\n/gm,'').replace(/^export /gm,'');
  const content = vm.runInNewContext(`${documentSource}\ncontent`,{
    div:() => element('div'),theme:() => ({colors:{}}),
    EmptyState:{new:(id,props) => element('empty',{id,...props})},
    StatusBar:function() {assert.fail('an empty document must not reserve a status bar');},
    NativeContextMenu:{new:() => assert.fail('an empty path must not mount actions')},
    DocumentSurface:{new:() => assert.fail('an empty document must not mount an editor')},
  });
  const view = {text:{file_empty:'Open a file'},documents:{current:() => null},
    pathItems:() => assert.fail('an empty path has no captured target')};
  const result = content(view),nodes = all(result);
  assert.equal(result.props.id,'file-document');
  assert.equal(result.items.length,1);
  assert.deepEqual(Array.from(result.items[0].props.flex_1),[]);
  assert.equal(result.items[0].items[0].kind,'empty');
  assert.equal(nodes.some(node => node?.props?.id === 'document-path'),false);
  assert.equal(nodes.some(node => node?.props?.id === 'file-toolbar'),false);
});

test('open documents retain path actions and native status content', async () => {
  const documentSource = (await readFile(new URL('../dev.sailry.platform/desktop/document-view.js',import.meta.url),'utf8'))
    .replace(/^import .*;\n/gm,'').replace(/^export /gm,'');
  const content = vm.runInNewContext(`${documentSource}\ncontent`,{
    div:() => element('div'),theme:() => ({colors:{}}),
    IconButton:{new:(id,props) => element('button',{id,...props})},
    StatusBar:function() {return element('status');},
    NativeContextMenu:{new:(id,props) => element('menu',{id,...props})},
    DocumentSurface:{new:(id,props) => element('editor',{id,...props})},
    EmptyState:{new:() => assert.fail('an open document must not mount an empty state')},
  });
  const actions = [{id:'copy',label:'Copy path'}];
  const document = {id:'draft',path:'notes.txt',position:{line:2,column:4},language:'Plain text',saving:true};
  const tree = content({text:{files_saving:'Saving',files_cursor_position:'Line {line}, Column {column}'},
    documents:{current:() => document},pathItems:target => {assert.equal(target,document);return actions;}});
  const nodes = all(tree),bar = nodes.find(node => node?.kind === 'status');
  const status = [bar.props.left_content,bar.props.right_content].flatMap(all);
  assert.ok(nodes.some(node => node?.props?.id === 'file-toolbar'));
  assert.deepEqual(Array.from(bar.props.h_8),[]);
  assert.equal(status.find(node => node?.props?.id === 'document-path-label').items[0],'notes.txt');
  assert.deepEqual(Array.from(status.find(node => node?.kind === 'menu').props.items),actions);
  assert.ok(nodes.some(node => node?.items?.includes('Saving')));
  assert.ok(status.some(node => node?.items?.includes('Line 2, Column 4')));
  assert.ok(status.some(node => node?.items?.includes('UTF-8')));
  assert.ok(status.some(node => node?.items?.includes('Plain text')));
});

test('populated explorers retain native tree insets and header control states', () => {
  const render = vm.runInNewContext(`${source}\nrender`,{
    div:() => element('div'),theme:() => ({colors:{}}),
    PanelHeader:{new:id => element('header',{id})},
    IconButton:{new:(id,props) => element('icon',{id,...props})},Menu:{new:(id,props) => element('menu',{id,...props})},
    NativeContextMenu:{new:id => element('context',{id})},
    ResourceTree:{new:(id,props) => element('tree',{id,...props})},
    TextField:{new:id => element('text',{id})},Toggle:{new:id => element('toggle',{id})},
  });
  const view = {text:{},search:null,
    explorer:{pages:new Map([['',{loaded:true,entries:[{path:'notes.txt'}]}]]),connected:true,
      directory:() => '',items:() => [],paths:() => [],current:null},
    actions:{create:() => [],tree:() => [],treeMenu:() => []},
  };
  const nodes = all(render(view));
  const inset = nodes.find(node => node?.items?.some(item => item?.kind === 'tree'));
  assert.ok(inset);
  assert.deepEqual(Array.from(inset.props.p_2),[]);
  assert.deepEqual(Array.from(inset.props.flex_1),[]);
  assert.equal(nodes.find(node => node?.props?.id === 'file-create-menu').props.size,'medium');
  assert.equal(nodes.find(node => node?.props?.id === 'file-search-toggle').props.size,'medium');
  assert.equal(nodes.find(node => node?.props?.id === 'file-search-toggle').props.selected,false);
  view.search = {};
  assert.equal(all(render(view)).find(node => node?.props?.id === 'file-search-toggle').props.selected,true);
});
