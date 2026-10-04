import {readFile} from "node:fs/promises";
import vm from "node:vm";

export const id = index => `00000000-0000-0000-0000-${String(index).padStart(12,"0")}`;
export const plain = value => JSON.parse(JSON.stringify(value));
export async function fixture(project=id(1)) {
  const sources=await Promise.all(["records","dispatch","mutations","tools","management"].map(async name =>
    (await readFile(new URL(`../dev.sailry.platform/host/${name}.js`,import.meta.url),"utf8"))
      .replace(/^import .*;\n/gm,"").replace(/^export \{.*;\n/gm,"").replace(/^export /gm,"")));
  const values=new Map(), schedules=new Map(), handlers=new Map(), requests=[], transactions=[];
  const config={assistant:null,resource:null,provider:id(3),model:"chosen",effort:"high",mode:"code",permission:"ask",credential:null};
  let turn=null, call=id(10);
  const setup={config,session:{config},values,schedules,handlers,requests,transactions,
    project,worktree:id(2),nextCall(value){call=value;},turn(){return turn;}};
  const runtime={context:()=>({package:{name:"scheduled-tasks"},invocation:call,worktree:setup.worktree}),
    callId:()=>call,readSession:async()=>plain(setup.session),readTurnState:()=>turn,stageTurnState:value=>{turn=plain(value);},
    getValue:async key=>plain(values.get(key)??{revision:"0",present:false,value:null}),
    listKeys:async(prefix,after,limit)=>{
      const keys=[...values.keys()].filter(key=>key.startsWith(prefix)&&(!after||key>after)).sort();
      return {keys:keys.slice(0,limit),after:keys.length>limit?keys[limit-1]:null};
    },
    readProjectCatalog:async()=>({projects:[{id:id(1)},{id:id(4)}],worktrees:[{id:id(2),project:id(1)}]}),
    prepareRequest:command=>{requests.push(plain(command));return requests.length-1;},
    prepareTransaction:operations=>{transactions.push(plain(operations));return "management";},
    completeRequest:async request=>{
      const command=requests[request];
      if(command?.kind==="resolve_plugin_model")return {Ok:{kind:"session_config",data:plain(command.data.config)}};
      const action=command?.data?.action;
      if(!["list_schedules","list_handlers"].includes(action?.kind))throw new Error("Unexpected dispatch action");
      const data=action.kind==="list_schedules"?[...schedules.values()]:[...handlers.values()];
      return {Ok:{data:{data:plain(data)}}};
    },forgetRequest(){},
  };
  setup.api=vm.runInNewContext(`${sources.join("\n")}\n({initialize,scheduled_tasks,saveOperations,removeOperations,transactionResult})`,runtime);
  setup.initialize=(config=setup.config)=>setup.api.initialize({project:setup.project,tools:["scheduled_tasks"],config:plain(config)});
  setup.plan=args=>setup.api.scheduled_tasks({arguments:args,step:0});
  setup.finish=(reply,outcome)=>setup.api.scheduled_tasks({step:1,state:reply.state,outcome});
  setup.commit=async reply=>{
    const operations=plain(reply.call.arguments.operations), output=[];
    // Validate the entire fixture transaction before applying any operation.
    for(const operation of operations) {
      const data=operation.data;
      if(operation.kind==="write"||operation.kind==="remove") {
        if((values.get(data.key)?.revision??"0")!==data.expected_revision) return setup.finish(reply,{isError:true,error:{code:"revision_conflict",message:"Task changed"}});
      } else {
        const command=data.data, store=data.kind.includes("schedule")?schedules:handlers;
        const key=command.id??command.name;
        if(String(store.get(key)?.revision??0)!==(command.revision??command.expected_revision)) return setup.finish(reply,{isError:true,error:{code:"revision_conflict",message:"Schedule changed"}});
      }
    }
    for(const operation of operations) {
      const data=operation.data;
      if(operation.kind==="write"||operation.kind==="remove") {
        const entry={key:data.key,revision:String(BigInt(data.expected_revision)+1n),present:operation.kind==="write",value:data.value??null};
        values.set(data.key,entry);output.push({kind:"plugin_value",data:entry});
      } else {
        const command=data.data, store=data.kind.includes("schedule")?schedules:handlers;
        const key=command.id??command.name;
        if(data.kind.startsWith("remove"))store.delete(key);
        else store.set(key,{...command,revision:Number(command.revision)+1});
      }
    }
    return setup.finish(reply,{kind:"plugin_transaction",data:output});
  };
  setup.task=(index,project=setup.project)=>({id:id(index),name:"Review",prompt:"Read the project",project,
    worktree:project===null?null:setup.worktree,config:plain(config),enabled:false,queue:"default",
    timing:{kind:"once",data:{at_ms:Date.now()+3_600_000}}});
  setup.seed=item=>values.set(`task/${item.id}`,{present:true,revision:"1",value:plain(item)});
  await setup.initialize();
  return setup;
}
