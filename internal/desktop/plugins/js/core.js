(() => {
  const componentNames=["Accordion","AccordionItem","Attachment","Badge","Button","Checkbox","Collapsible","Diff","DropdownMenu","Editor","Field","Form","HorizontalRadioGroup","Icon","Label","Link","Pagination","Progress","Radio","Resizable","ResizablePanel","ShimmerText","Spinner","StatusBar","Switch","Tab","TabBar","Tag","Text","Toggle","VForm"];
  const states=["EditorState","DiffState","InputState"];
  const components={};
  const methods={
    Button:{secondary:"flag",success:"flag",warning:"flag",link:"flag"},
    TabBar:{selected_index:"index",menu:"boolean"},
    HorizontalRadioGroup:{selected_index:"index"},
    Field:{required:"boolean"},
    Pagination:{current_page:"positive",total_pages:"positive",visible_pages:"positive"},
    Accordion:{on_toggle:"callback"},
    AccordionItem:{title:"element"},
    Collapsible:{motion_id:"string"},
    Diff:{header_visible:"boolean"},
    StatusBar:{left_content:"element",right_content:"element"},
  };
  for(const name of componentNames) {
    const construct=function(...arguments) {
      const props={arguments:arguments.map(__desktop.serial)};
      if(arguments.length && typeof arguments[0]==="string") props.id=arguments[0];
      if(["Text","Label","ShimmerText"].includes(name)) {props.label=String(arguments[0]??"");delete props.id;}
      if(arguments[0]?.__coreState) props.state=__desktop.serial(arguments[0]);
      if(name==="Diff" && arguments[0]?.__coreState) props.state=__desktop.serial(arguments[0]);
      const handle=__desktop.create(name,props,"gpui-component");
      const element=__gpui.__element(handle);
      for(const [method,schema] of Object.entries(methods[name]??{})) {
        element[method]=function(...values) {
          const value=values[0];
          if(schema==="element") {
            if(typeof value?.__id!=="number") throw new TypeError(name+"."+method+" expects an element");
            __desktop.add(handle,value.__id,method);
          } else {
            if(schema==="boolean" && typeof value!=="boolean") throw new TypeError(name+"."+method+" expects a boolean");
            if(schema==="string" && typeof value!=="string") throw new TypeError(name+"."+method+" expects a string");
            if((schema==="positive" || schema==="index") && (!Number.isSafeInteger(value) || value<(schema==="positive"?1:0))) throw new TypeError(name+"."+method+" expects a "+(schema==="positive"?"positive":"non-negative")+" integer");
            if(schema==="callback" && typeof value!=="function") throw new TypeError(name+"."+method+" expects a callback");
            __desktop.apply(handle,method,values);
          }
          return this;
        };
      }
      return element;
    };
    construct.new=construct;
    components[name]=construct;
  }
  for(const name of states) {
    const construct=function(...arguments) {
      const first=arguments[0];
      const data=first && typeof first==="object"?{...first}:{value:typeof first==="string"?first:""};
      if(name==="DiffState") {
        data.patch=String(arguments[0]??"");data.mode=arguments[1]??"unified";
        if(!["unified","split"].includes(data.mode)) throw new TypeError("DiffState mode expects unified or split");
      }
      return __desktop.state(name,data);
    };
    construct.new=construct;
    components[name]=construct;
  }
  globalThis.__desktopComponents=Object.freeze(components);
  globalThis.__host_function=(module,name)=>(...arguments)=>__desktop_host(module,name,JSON.stringify(arguments));
  globalThis.__host_async_call=(module,name,...arguments)=>__desktop_async(module,name,JSON.stringify(arguments));
  if(typeof globalThis.console==="undefined") globalThis.console={log(){},info(){},warn(){},error(){},debug(){}};
})();
