import {compact,amount,integer} from './format.js';
const pad=(value,width=2)=>String(value).padStart(width,'0');
function timestamp(value) {
  const date=new Date(Number(value));
  if(!Number.isFinite(date.getTime()))return null;
  const label=`${date.getFullYear()}/${pad(date.getMonth()+1)}/${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
  const offset=-date.getTimezoneOffset(),zone=`${offset<0?'-':'+'}${pad(Math.floor(Math.abs(offset)/60))}:${pad(Math.abs(offset)%60)}`;
  return [label,`${label}.${pad(date.getMilliseconds(),3)} ${zone}`];
}
export function requestRows(items,text) {
  const unknown=text.composer_metric_unknown;
  const duration=value=>value==null?unknown:Number(value)<1000000?text.usage_duration_ms.replace('%{value}',(Number(value)/1000).toFixed(1))
    :text.usage_duration_value.replace('%{value}',(Number(value)/1000000).toFixed(2));
  const exact=value=>value==null?unknown:String(value);
  const line=(value,icon,tone)=>({text:value,...icon?{icon,tone}: {}});
  const pair=(primary,secondary)=>({primary,secondary});
  const rows=[],cells=[],details=[],row_ids=[];
  for(const request of items) {
    const time=timestamp(request.position.timestamp_ms)??[unknown,unknown],tokens=['input','output','cached_input','reasoning'].map(key=>request.tokens?.[key]);
    const common=[duration(request.first_token_us),duration(request.elapsed_us),amount(request.usd_micros)??unknown];
    const scope=request.scope_name||text.usage_unassigned;
    const tokenValues=tokens.map(value=>compact(value)??unknown),tokenDetails=tokens.map(exact);
    const timing=[request.first_token_us,request.elapsed_us].map(value=>value==null?unknown:text.usage_duration_us.replace('%{value}',value));
    const micros=integer(request.usd_micros),cost=micros==null?unknown:`${micros/1000000n}.${String(micros%1000000n).padStart(6,'0')}`;
    const labels=[['model',request.model,'provider',request.provider_name],['project',scope,'time',time[1]],
      ['input',tokenDetails[0],'output',tokenDetails[1]],['cached',tokenDetails[2],'reasoning',tokenDetails[3]],
      ['first_token',timing[0],'duration',timing[1]]];
    rows.push([...labels.map(([,primary,,secondary])=>`${primary} · ${secondary}`),cost]);
    details.push([...labels.map(([first,primary,second,secondary])=>`${text[`usage_column_${first}`]}: ${primary}\n${text[`usage_column_${second}`]}: ${secondary}`),cost]);
    cells.push([
      pair(line(request.model),line(request.provider_name)),pair(line(scope),line(time[0])),
      pair(line(tokenValues[0],'reicon:arrows/arrow-down','chart_2'),line(tokenValues[1],'reicon:arrows/arrow-up','chart_4')),
      pair(line(tokenValues[2],'reicon:ui/hard-drive','muted'),line(tokenValues[3],'reicon:devices/cpu','muted')),
      pair(line(common[0],'reicon:devices/lightning','muted'),line(common[1],'reicon:time/clock','muted')),
      {primary:line(common[2])}
    ]);
    row_ids.push(`${request.position.node}:${request.position.timestamp_ms}:${request.position.sequence}`);
  }
  return {rows,cells,details,row_ids};
}
