globalThis.__desktop_async=(module,name,data)=>__desktop.capturePromise(__desktop_async_value(module,name,data));
globalThis.__desktop = (() => {
  let generation = 0, nextNode = 0, nextOrdinal = 0, nextCallback = 0, nextTask = 0;
  let phase = "none", closed = false, failure = null, object = null, dirty = true, currentView = null;
  let nodes = new Map(), callbacks = new Map(), tasks = new Map(), entities = new Map();
  let registered = new Set(), consumed = new Set(), rootSnapshot = null, focused = null;
  const focusHandles=new Set();
  let viewport = {width:1280,height:820}, themeRevision = 0;
  let theme = {colors:{background:"#ffffffff",foreground:"#171717ff",surface:"#f5f5f5ff",border:"#dededeff",muted_foreground:"#737373ff",primary:"#2563ebff",destructive:"#dc2626ff",success:"#16a34aff",warning:"#ca8a04ff"},spacing:{},radius:{}};
  const states = new Map();
  const check = () => {if(closed) throw new Error("plugin view is closed");};
  const liveView=view=>view===null || view===object || [...entities.values()].some(entity=>entity.object===view);
  const node = id => {
    check();
    const found = nodes.get(id);
    if (!found) throw new Error("element belongs to a retired render generation");
    return found;
  };
  const create = (kind,props={},module="gpui-base",text="") => {
    if(phase!=="render" && phase!=="layout") throw new Error("elements may only be built during render or layout");
    const id=++nextNode;
    nodes.set(id,{ID:id,Module:module,Export:kind,Text:text,Props:{...props,__ordinal:++nextOrdinal},Styles:[],Children:[],Slots:{},Callbacks:{}});
    return id;
  };
  const retain = handler => {
    if(typeof handler!=="function") throw new TypeError("event handler must be a function");
    const id=++nextCallback;
    callbacks.set(id,{handler,generation,view:currentView});
    return {generation,id};
  };
  const add = (parent,child,slot) => {
    const target=node(parent), value=node(child);
    if(consumed.has(child)) throw new Error("an element cannot be attached more than once");
    consumed.add(child);
    if(slot) (target.Slots[slot]??=[]).push(value); else target.Children.push(value);
  };
  const serial = value => {
    if(typeof value==="function") return {__callback:retain(value)};
    if(Array.isArray(value)) return value.map(serial);
    if(value && typeof value==="object") {
      if(value.__coreState) return {__coreState:value.__coreState,...states.get(value.__coreState).data};
      return Object.fromEntries(Object.entries(value).map(([key,item])=>[key,serial(item)]));
    }
    return value===undefined?null:value;
  };
  const apply = (id,name,args) => {
    const target=node(id);
    if(["content","trigger","input","decrement_button","increment_button","image","fallback","header","footer","panel"].includes(name)) {
      add(id,args[0],name); return;
    }
    if(name==="on_action") {
      if(typeof args[0]!=="string" || args[0].length===0) throw new TypeError("on_action(action, handler) expects a non-empty action name");
      if(typeof args[1]!=="function") throw new TypeError("on_action(action, handler) expects a function second");
      target.Callbacks["on_action:"+args[0]]=retain(args[1]);
      return;
    }
    if(name.startsWith("on_") || name==="token") {
      const handler=args.find(value=>typeof value==="function");
      if(!handler) throw new TypeError(name+" expects a callback");
      target.Callbacks[name]=retain(handler);
      return;
    }
    if(name==="id") target.Props.id=String(args[0]);
    target.Styles.push({name,arguments:args.map(serial)});
  };
  const invoke = (body,args=[],kind="event",async=false,view=currentView,input=false) => {
    check();
    if(!liveView(view)) throw new Error("retained view is released");
    const previous=phase, previousView=currentView, token={active:true,stopped:false};
    phase=kind;currentView=view;
    const context=makeContext(token,async,view);
    try {const value=body(...args,context);return input?token.stopped:value;}
    finally {token.active=false;phase=previous;currentView=previousView;}
  };
  const makeContext = (token,async,view=currentView) => {
    const valid=()=>{
      check();
      if(!liveView(view)) throw new Error("retained view is released");
      if(!async && !token.active) throw new Error("call-scoped Context is stale");
    };
    const context=__gpui.__context_members(valid);
    context.notify=entity=>{
      valid();
      if(phase==="render"||phase==="layout") throw new Error("cx.notify is not allowed during render or layout");
      if(entity && !entities.has(entity.__handle)) throw new Error("unknown retained view");
      dirty=true;__desktop_notify();
    };
    context.phase=()=>{valid();return async&&phase==="none"?"task":phase;};
    context.stop_propagation=()=>{valid();token.stopped=true;};
    context.propagate=()=>{valid();token.stopped=false;};
    return context;
  };
  const taskOwner=options=>{
    const owner=options?.owner;
    if(owner===undefined) return currentView;
    if(owner===null || owner===currentView) return owner;
    throw new TypeError("opts.owner must be the view that is running, or null");
  };
  const taskHandle=(id,task)=>({
    cancel(){task.cancelled=true;if(tasks.delete(id) && task.handler) __desktop_cancel_timer(id);},
    is_done(){return !tasks.has(id);},
  });
  const timer=(milliseconds,handler,repeat=false,options)=>{
    check();
    if(typeof handler!=="function" || !Number.isFinite(milliseconds) || milliseconds<0) throw new TypeError("invalid timer");
    const id=++nextTask, task={handler,milliseconds,repeat,view:taskOwner(options),cancelled:false};
    tasks.set(id,task);__desktop_timer(id,milliseconds);
    return taskHandle(id,task);
  };
  const state=(kind,data={})=>{
    const id=++nextTask, handlers=new Map();
    states.set(id,{kind,data,handlers,view:currentView});
    const result={__coreState:id};
    result.read=()=>({...data});
    result.value=()=>data.value??"";
    result.text=()=>String(data.value??data.text??"");
    result.set_value=value=>{check();data.value=value;dirty=true;};
    result.set_text=result.set_value;
    result.set_placeholder=value=>{data.placeholder=String(value);};
    result.set_disabled=value=>{data.disabled=!!value;};
    result.set_language=value=>{data.language=String(value);};
    result.set_mode=value=>{data.mode=String(value);};
    result.on=(event,handler)=>{if(typeof handler!=="function") throw new TypeError("state event needs a callback");handlers.set(event,handler);return result;};
    result.focus=()=>{data.focused=true;};
    result.blur=()=>{data.focused=false;};
    result.is_focused=()=>!!data.focused;
    result.release=()=>states.delete(id);
    return Object.freeze(result);
  };
  const valueFrom = value => {
    if(typeof value==="string"||typeof value==="number"||typeof value==="boolean") return create("Text",{},"gpui-kit",String(value));
    if(value?.__entity) {
      const child=entities.get(value.__handle);
      if(!child) throw new Error("retained child view is unavailable");
      return valueFrom(invoke((cx)=>child.object.render(cx),[],"render",false,child.object));
    }
    if(typeof value?.__id!=="number") throw new TypeError("render must return an element, entity or string");
    return value.__id;
  };
  const exported={
    create,apply,add,state,serial,retain,
    capturePromise(promise) {
      const view=currentView;
      return promise.then(value=>liveView(view)?value:new Promise(()=>{}),error=>{
        if(!liveView(view)) return new Promise(()=>{});
        throw error;
      });
    },
    configure(names){registered=new Set(names);},
    component(module,name,id,props) {
      if(!registered.has(module+"/"+name)) throw new Error("native component "+module+"."+name+" is unavailable");
      if(module==="sailry/ui" && name==="PathBar" && (!Array.isArray(props?.segments) || !props.segments.every(segment=>typeof segment==="string")))
        throw new TypeError("PathBar segments must be an array of strings");
      return create(name,{...props,id},module);
    },
    mount(Class) {
      check();
      if(typeof Class!=="function" || !(Class.prototype instanceof __gpui.View)) throw new TypeError("desktop entry must default-export a View subclass");
      object=__construct(Class);
      if(typeof object.init==="function") invoke(cx=>object.init(undefined,cx),[],"event",true,object);
      dirty=true;
    },
    render() {
      check();
      if(!object) throw new Error("desktop entry has not finished loading");
      if(!dirty && rootSnapshot) return rootSnapshot;
      generation++;nextOrdinal=0;nodes=new Map();consumed=new Set();
      const previous=phase;phase="render";
      try {
        const root=valueFrom(invoke(cx=>object.render(cx),[],"render",false,object));
        rootSnapshot=JSON.stringify(node(root));
        dirty=false;
        for(const [id,callback] of callbacks) if(callback.generation+1<generation) callbacks.delete(id);
        return rootSnapshot;
      } finally {phase=previous;}
    },
    dispatch(owner,id,event) {
      const callback=callbacks.get(id);
      if(!callback || callback.generation!==owner) throw new Error("event callback belongs to a retired render generation");
      return invoke(callback.handler,[event??{}],"event",false,callback.view);
    },
    dispatchInput(owner,id,event) {
      const callback=callbacks.get(id);
      if(!callback || callback.generation!==owner || !liveView(callback.view)) return false;
      return invoke(callback.handler,[event??{}],"event",false,callback.view,true);
    },
    timer(id) {
      const task=tasks.get(id);
      if(!task) return;
      if(!task.repeat) tasks.delete(id);
      if(task.cancelled || !liveView(task.view)) {tasks.delete(id);return;}
      invoke(task.handler,[],"task",true,task.view);
      if(task.repeat && tasks.has(id)) __desktop_timer(id,task.milliseconds);
    },
    stateChange(id,value) {
      const found=states.get(id);
      if(!found) throw new Error("control state is released");
      found.data.value=value;
      dirty=true;
      for(const name of ["change","input"]) {
        const handler=found.handlers.get(name);
        if(handler) invoke(handler,[{value,text:String(value)}],"event",false,found.view);
      }
    },
    setViewport(value){if(viewport.width!==value.width || viewport.height!==value.height) dirty=true;viewport=value;},
    setTheme(value){if(JSON.stringify(theme)!==JSON.stringify(value)) {theme=value;themeRevision++;dirty=true;}},
    fail(error){failure=error instanceof Error?error:new Error(String(error));},
    check(){check();if(failure) {const error=failure;failure=null;throw error;}},
    close(){closed=true;callbacks.clear();tasks.clear();entities.clear();states.clear();nodes.clear();object=null;rootSnapshot=null;},
    context:makeContext,
    focused(id){return focused===id;},
  };
  globalThis.__div=()=>create("Div",{},"gpui-kit");
  globalThis.__h_flex=()=>create("HFlex");
  globalThis.__v_flex=()=>create("VFlex");
  globalThis.__text=value=>create("Text",{},"gpui-kit",String(value));
  globalThis.__text_view=(id,text,format)=>create("TextView",{id,format},"gpui-base",text);
  globalThis.__svg=value=>create("Svg",{path:value},"gpui-kit");
  globalThis.__image=value=>create("Image",{path:value},"gpui-kit");
  const lazyList=(kind,axis,id,count,sizes,getKey,render)=>{
    const handle=create(kind,{id,item_count:count,item_sizes:sizes,axis});
    node(handle).Callbacks.get_key=retain(getKey);
    node(handle).Callbacks.render=retain(render);
    return handle;
  };
  globalThis.__v_virtual_list=(id,count,sizes,getKey,render)=>lazyList("VirtualList","vertical",id,count,sizes,getKey,render);
  globalThis.__h_virtual_list=(id,count,sizes,getKey,render)=>lazyList("VirtualList","horizontal",id,count,sizes,getKey,render);
  globalThis.__list=(id,count,getKey,render)=>lazyList("MeasuredList","vertical",id,count,null,getKey,render);
  globalThis.__uniform_list=(id,count,getKey,render)=>lazyList("UniformList","vertical",id,count,null,getKey,render);
  const constructors={
    button:"Button",link:"Link",checkbox:"Checkbox",switch:"Switch",tabs:"Tabs",tab:"Tab",progress:"Progress",
    accordion:"Accordion",accordion_item:"AccordionItem",accordion_header:"AccordionHeader",accordion_panel:"AccordionPanel",accordion_trigger:"AccordionTrigger",
    pagination:"Pagination",avatar:"Avatar",avatar_image:"AvatarImage",avatar_fallback:"AvatarFallback",progress_track:"ProgressTrack",progress_indicator:"ProgressIndicator",
    radio:"Radio",toggle:"Toggle",radio_group:"RadioGroup",toggle_group:"ToggleGroup",table:"Table",table_header:"TableHeader",table_body:"TableBody",table_caption:"TableCaption",
    table_row:"TableRow",table_head:"TableHead",table_cell:"TableCell",h_resizable:"HResizable",v_resizable:"VResizable",resizable_panel:"ResizablePanel",
    collapsible:"Collapsible",popover:"Popover",hover_card:"HoverCard",popup:"Popup",select:"Select",combobox:"Combobox",date_picker:"DatePicker",scrollbar:"Scrollbar",
    input_element:"Input",textarea_element:"Textarea",number_input:"NumberInput",slider_element:"Slider",slider_track_element:"SliderTrack",slider_indicator_element:"SliderIndicator",slider_thumb_element:"SliderThumb",otp_element:"OtpInput",
  };
  for(const [name,kind] of Object.entries(constructors)) globalThis["__"+name]=(...args)=>create(kind,{id:args[0],arguments:args});
  globalThis.__component=exported.component;
  globalThis.__apply=apply;
  globalThis.__attach=(parent,child)=>add(parent,child);
  globalThis.__applyNullaryStyle=(id,index)=>apply(id,__nullaryStyles[index],[]);
  globalThis.__applyParamStyle=(id,index,value)=>apply(id,__paramStyles[index],[value]);
  globalThis.__unknown=name=>{throw new TypeError("unknown element method "+name);};
  globalThis.__theme_revision=()=>themeRevision;
  globalThis.__theme_snapshot=()=>JSON.stringify(theme);
  globalThis.__spawn=(body,options)=>{
    check();
    if(phase==="render" || phase==="layout") throw new Error("tasks cannot start during render or layout");
    if(typeof body!=="function") throw new TypeError("cx.spawn expects a function");
    const id=++nextTask, task={cancelled:false,view:taskOwner(options)};
    tasks.set(id,task);
    try {Promise.resolve(invoke(body,[],"task",true,task.view)).catch(error=>{if(!task.cancelled&&!closed&&liveView(task.view)) exported.fail(error);}).finally(()=>tasks.delete(id));}
    catch(error){tasks.delete(id);throw error;}
    return taskHandle(id,task);
  };
  globalThis.__async_cx=()=>makeContext({active:true},true);
  globalThis.__sleep=(milliseconds=0)=>new Promise(resolve=>timer(milliseconds,()=>resolve()));
  globalThis.__timer_after=(milliseconds,handler,options)=>timer(milliseconds,handler,false,options);
  globalThis.__timer_every=(milliseconds,handler,options)=>timer(milliseconds,handler,true,options);
  globalThis.__window_viewport_size=()=>viewport;
  globalThis.__window_rem_size=()=>16;
  globalThis.__window_line_height=()=>24;
  globalThis.__window_bounds=()=>({origin:{x:0,y:0},size:viewport});
  globalThis.__window_mouse_position=()=>({x:0,y:0});
  globalThis.__window_appearance=()=>theme.dark?"dark":"light";
  globalThis.__window_is_active=()=>true;
  globalThis.__window_is_fullscreen=()=>false;
  globalThis.__window_is_maximized=()=>false;
  globalThis.__window_refresh=()=>{dirty=true;__desktop_notify();};
  globalThis.__path=(fill,kind,values,opacity,colorSpace,width)=>create("Path",{fill,kind,color:values[0],opacity,colorSpace,width},"gpui-kit");
  globalThis.__state=(id,name)=>{
    const child=create("Style");
    (node(id).Slots[name]??=[]).push(node(child));
    return child;
  };
  globalThis.__focus_handle_new=()=>{const id=++nextTask;focusHandles.add(id);return id;};
  globalThis.__focus_focus=id=>{if(!focusHandles.has(id)) throw new Error("focus handle is released");__desktop_focus("focus",id);focused=id;dirty=true;__desktop_notify();};
  globalThis.__focus_is_focused=id=>focusHandles.has(id)&&__desktop_focus("focused",id);
  globalThis.__focus_release=id=>{__desktop_focus("release",id);focusHandles.delete(id);if(focused===id) focused=null;};
  globalThis.__bind_keys=bindings=>bindings.map(binding=>({...binding}));
  globalThis.__view_new=(Class,props)=>{
    if(phase==="render"||phase==="layout") throw new Error("retained views cannot be created during render");
    const id=++nextTask, instance=__construct(Class);
    entities.set(id,{object:instance,props,parent:currentView});
    if(instance.init) invoke(cx=>instance.init(props,cx),[],"event",true,instance);
    return id;
  };
  globalThis.__view_set_props=(id,props)=>{
    if(phase==="render"||phase==="layout") throw new Error("retained views cannot be updated during render or layout");
    const found=entities.get(id);
    if(!found) throw new Error("retained view is released");
    const restore=__checkpoint_view(found.object), oldTasks=new Set(tasks.keys()),oldStates=new Set(states.keys()),oldEntities=new Set(entities.keys());
    try {
      if(found.object.update) invoke(()=>found.object.update(props),[],"event",false,found.object);
      found.props=props;
    } catch(error) {
      restore();
      for(const [handle,task] of tasks) if(!oldTasks.has(handle)) taskHandle(handle,task).cancel();
      for(const handle of states.keys()) if(!oldStates.has(handle)) states.delete(handle);
      for(const handle of entities.keys()) if(!oldEntities.has(handle)) __view_release(handle);
      throw error;
    }
    dirty=true;
  };
  globalThis.__view_update=__view_set_props;
  globalThis.__view_release=id=>{
    if(phase==="render"||phase==="layout") throw new Error("retained views cannot be released during render or layout");
    const found=entities.get(id);
    if(!found) return false;
    entities.delete(id);
    for(const [handle,child] of entities) if(child.parent===found.object) __view_release(handle);
    for(const [handle,task] of tasks) if(task.view===found.object) taskHandle(handle,task).cancel();
    for(const [handle,state] of states) if(state.view===found.object) states.delete(handle);
    for(const [handle,callback] of callbacks) if(callback.view===found.object) callbacks.delete(handle);
    dirty=true;return true;
  };
  globalThis.__child_view=id=>valueFrom({__entity:true,__handle:id});
  globalThis.__input_state_new=(placeholder,value)=>state("InputState",{value:value??"",placeholder}).__coreState;
  globalThis.__textarea_state_new=(placeholder,value,rows)=>state("TextareaState",{value:value??"",placeholder,rows}).__coreState;
  globalThis.__calendar_state_new=()=>state("CalendarState",{}).__coreState;
  globalThis.__slider_state_new=(min,max,step,scale,value)=>state("SliderState",{min,max,step,scale,value}).__coreState;
  globalThis.__otp_state_new=(length,value,masked)=>state("OtpState",{length,value,masked}).__coreState;
  globalThis.__state_read=id=>{const found=states.get(id);if(!found) throw new Error("state is released");return found.data;};
  globalThis.__state_set_value=(id,value)=>exported.stateChange(id,value);
  globalThis.__state_release=id=>states.delete(id);
  for(const [prefix,kind] of [["input","Input"],["textarea","Textarea"],["slider","Slider"],["otp","OtpInput"]]) {
    globalThis["__"+prefix+"_element"]=id=>create(kind,{state:{__coreState:id,...__state_read(id)}});
    globalThis["__"+prefix+"_value"]=id=>__state_read(id).value;
    globalThis["__"+prefix+"_set_value"]=(id,value)=>exported.stateChange(id,value);
    globalThis["__"+prefix+"_release"]=id=>states.delete(id);
    globalThis["__"+prefix+"_on"]=(id,event,handler)=>{
      const found=states.get(id);if(!found) throw new Error("state is released");
      found.handlers.set(event,handler);
    };
    for(const property of ["step","min","max","masked","loading","rows","soft_wrap","submit_on_enter"])
      globalThis["__"+prefix+"_set_"+property]=(id,value)=>{__state_read(id)[property]=value;dirty=true;};
  }
  globalThis.__pagination_items=(current,total,visible)=>{
    if(!Number.isInteger(current)||!Number.isInteger(total)||total<0||current<1||visible<1) throw new TypeError("invalid pagination bounds");
    const start=Math.max(1,Math.min(current-Math.floor(visible/2),total-visible+1));
    return Array.from({length:Math.max(0,Math.min(total,visible))},(_,index)=>start+index);
  };
  const local=new Map(),session=new Map();
  globalThis.__storage_get=(temporary,key)=>(temporary?session:local).get(key)??null;
  globalThis.__storage_set=(temporary,key,value)=>(temporary?session:local).set(key,value);
  globalThis.__storage_remove=(temporary,key)=>(temporary?session:local).delete(key);
  globalThis.__storage_clear=temporary=>(temporary?session:local).clear();
  globalThis.__storage_length=temporary=>(temporary?session:local).size;
  globalThis.__storage_key=(temporary,index)=>[...(temporary?session:local).keys()][index]??null;
  globalThis.__storage_flush=temporary=>Promise.resolve();
  const nullary=[
    "flex","h_flex","v_flex","flex_row","flex_col","flex_row_reverse","flex_col_reverse","flex_wrap","flex_nowrap","block","inline_block","hidden","visible","invisible","relative","absolute",
    "items_start","items_center","items_end","items_stretch","items_baseline","justify_start","justify_center","justify_end","justify_between","justify_around","justify_evenly",
    "self_start","self_center","self_end","self_stretch","flex_1","flex_none","flex_initial","flex_auto","flex_grow","flex_shrink_0","flex_shrink","overflow_hidden","overflow_x_hidden","overflow_y_hidden",
    "size_full","w_full","h_full","w_auto","h_auto","min_w_0","min_h_0","max_w_full","max_h_full","rounded_none","rounded_sm","rounded_md","rounded_lg","rounded_xl","rounded_2xl","rounded_full",
    "text_xs","text_sm","text_base","text_lg","text_xl","text_2xl","text_3xl","text_left","text_center","text_right","text_ellipsis","truncate","whitespace_nowrap","whitespace_normal",
    "font_thin","font_extralight","font_light","font_normal","font_medium","font_semibold","font_bold","font_extrabold","font_black","italic","not_italic","underline","line_through","no_underline",
    "cursor_default","cursor_pointer","cursor_text","cursor_not_allowed","cursor_grab","cursor_grabbing","cursor_crosshair","shadow_none","shadow_sm","shadow_md","shadow_lg","shadow_xl","border_0","border_1","border_2","border_4","border_dashed","border_solid",
  ];
  const scale=[0,0.5,1,1.5,2,2.5,3,3.5,4,5,6,7,8,9,10,11,12,14,16,20,24,28,32,36,40,44,48,52,56,60,64,72,80,96];
  for(const prefix of ["p","px","py","pt","pb","pl","pr","m","mx","my","mt","mb","ml","mr","gap","gap_x","gap_y","w","h","size","min_w","min_h","max_w","max_h","top","bottom","left","right","inset"])
    for(const size of scale) nullary.push(prefix+"_"+String(size).replace(".","p"));
  for(const prefix of ["m","mx","my","mt","mb","ml","mr","top","bottom","left","right","inset"]) nullary.push(prefix+"_auto");
  for(const prefix of ["w","h","size","min_w","min_h","max_w","max_h"]) nullary.push(prefix+"_px");
  for(const prefix of ["border","border_t","border_r","border_b","border_l","border_x","border_y"])
    for(const width of [0,1,2,4,8]) nullary.push(prefix+"_"+width);
  globalThis.__nullaryStyles=[...new Set(nullary)];
  globalThis.__nullaryStyleIndexes=__nullaryStyles.map((_,index)=>index);
  globalThis.__paramStyles=["w","h","size","min_w","min_h","min_size","max_w","max_h","max_size","p","px","py","pt","pb","pl","pr","m","mx","my","mt","mb","ml","mr","inset","top","bottom","left","right","gap","gap_x","gap_y","flex_grow","flex_shrink","flex_basis","bg","shadow","text_color","text_bg","text_size","font_family","font_weight","line_height","opacity","border","border_t","border_b","border_l","border_r","border_x","border_y","border_color","rounded","rounded_t","rounded_b","rounded_l","rounded_r","rounded_tl","rounded_tr","rounded_bl","rounded_br"];
  globalThis.__behaviorNames=["on_click","on_link_click","on_mouse_move","on_hover","on_key_down","on_key_up","on_modifiers_changed","on_mouse_down","on_mouse_up","on_mouse_down_out","on_scroll_wheel","on_pinch","on_action","key_context","aria_level","keep_mounted","on_item_click","on_item_secondary_click","on_visible_range_change","on_change","token","on_token_click","on_token_hover","on_open_change","on_confirm","on_dismiss","on_step","disabled","selectable","scrollable","selected","checked","accessibility_label","tooltip","role","aria_selected","aria_active_descendant","window_control_area","tab_index","tab_stop","href","id","overflow_scroll","overflow_x_scroll","overflow_y_scroll","overflow_scrollbar","overflow_x_scrollbar","overflow_y_scrollbar","viewport_from_layout","controls_right","on_resize","set_position","pressed","start","value","indeterminate","row_count","column_count","open","default_open","overlay_closable","with_item_to_measure_index","with_follow_mode","label","title","description","icon","size","variant","small","xsmall","large","primary","ghost","outline","danger","loading","compact","text","color","bordered","closable","appearance","checked","disabled","multiple","collapsible","resizable","default_size","min_size","max_size","gap","axis","position","closeable","line_numbers","language","placeholder","show_whitespaces","read_only","soft_wrap","auto_grow"];
  globalThis.__anchorNames=["top_left","top_center","top_right","left_center","right_center","bottom_left","bottom_center","bottom_right"];
  return exported;
})();
