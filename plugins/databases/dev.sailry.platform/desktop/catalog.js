import {browseDatabase} from 'sailry/connections';
export class Catalog {
  constructor(profile,report=()=>{}) { this.profile=profile; this.report=report; this.names=[]; this.tables=new Map(); this.expanded=new Set(); this.loading=new Set(); this.failed=new Set(); this.database=null; this.current=null; }
  async load(database,cx,reset=false) {
    const key=database ?? '';
    if (this.loading.has(key)) return;
    this.loading.add(key); this.failed.delete(key); cx.notify();
    try {
      const outcome=await browseDatabase(this.profile.id,this.profile.revision,database);
      if (outcome.kind !== 'catalog') throw new Error('invalid catalog response');
      const catalog=outcome.data;
      if(reset){this.tables.clear();this.names=[];this.expanded.clear();this.current=null;}
      if (catalog.kind === 'databases') this.names=catalog.data;
      else {
        this.tables.set(catalog.data.database,catalog.data.tables);
        if (!this.names.includes(catalog.data.database)) this.names.push(catalog.data.database);
      }
    } catch (error) { this.failed.add(key);this.report(error); }
    finally { this.loading.delete(key); cx.notify(); }
  }
  items(text,menu) {
    return this.names.map((name,index)=>({id:`db:${index}`,label:name,expanded:this.expanded.has(name),icon:'database',children:
      this.tables.has(name) ? this.tables.get(name).length ? this.tables.get(name).map((table,row)=>({id:`table:${index}:${row}`,
        label:table.schema === 'main' || table.schema === name ? table.name : `${table.schema}.${table.name}`,icon:'table',menu:menu(table,name)}))
        : [{id:`empty:${index}`,label:text.db_tables_empty,disabled:true}]
      : [{id:`${this.failed.has(name)?'retry':'loading'}:${index}`,label:text[this.failed.has(name)?'refresh':'db_catalog_loading'],disabled:!this.failed.has(name)}]}));
  }
  target(id) {
    const [kind,index,row]=id.split(':');
    const database=this.names[Number(index)];
    if (database === undefined) return null;
    return kind === 'table' ? {database,table:this.tables.get(database)?.[Number(row)]} : {database};
  }
}
