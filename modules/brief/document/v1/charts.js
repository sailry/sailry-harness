(() => {
  const bytes = value => new TextEncoder().encode(value).length;
  const object = (value,keys) => value && typeof value === 'object' && !Array.isArray(value)
    && Object.keys(value).every(key => keys.includes(key));
  const label = value => typeof value === 'string' && bytes(value) <= 256 && !/[\x00-\x1f\x7f]/.test(value);
  const number = value => typeof value === 'number' && Number.isFinite(value) && Math.abs(value) <= 1e12;
  const points = data => Array.isArray(data) && data.length > 0 && data.length <= 500
    && data.every(point => object(point,['label','value']) && label(point.label) && number(point.value));

  function graph(nodes,links) {
    if (!Array.isArray(nodes) || !Array.isArray(links) || !nodes.length || nodes.length > 64 || !links.length || links.length > 128) return false;
    const ids = new Map(),edges = nodes.map(() => []),degrees = nodes.map(() => 0);
    for (const [index,node] of nodes.entries()) {
      if (!object(node,['id','label']) || !label(node.id) || !node.id || !label(node.label) || ids.has(node.id)) return false;
      ids.set(node.id,index);
    }
    for (const link of links) {
      if (!object(link,['source','target','value']) || !ids.has(link.source) || !ids.has(link.target)
        || link.source === link.target || !number(link.value) || link.value <= 0) return false;
      const source = ids.get(link.source),target = ids.get(link.target);
      edges[source].push(target);degrees[target]++;
    }
    const ready = degrees.flatMap((degree,index) => degree === 0 ? [index] : []);
    let visited = 0;
    while (ready.length) {
      const index = ready.pop();visited++;
      for (const target of edges[index]) if (--degrees[target] === 0) ready.push(target);
    }
    return visited === nodes.length;
  }

  function parse(source) {
    if (typeof source !== 'string' || bytes(source) > 65536) throw Error('Chart exceeds the supported size');
    const chart = JSON.parse(source);
    if (!object(chart,['type','title','data','nodes','links']) || !label(chart.title ?? '')) throw Error('Invalid chart');
    const {type,data,nodes,links} = chart;
    let valid = false;
    if (['line','bar','area','pie','radar'].includes(type)) {
      valid = object(chart,['type','title','data']) && points(data);
      if (type === 'pie' || type === 'radar') valid = valid && data.every(point => point.value >= 0)
        && data.some(point => point.value > 0) && (type !== 'radar' || data.length >= 3);
    } else if (type === 'candlestick') {
      valid = object(chart,['type','title','data']) && Array.isArray(data) && data.length > 0 && data.length <= 500
        && data.every(point => object(point,['label','open','high','low','close']) && label(point.label)
          && [point.open,point.high,point.low,point.close].every(number)
          && point.low <= Math.min(point.open,point.close) && point.high >= Math.max(point.open,point.close));
    } else if (type === 'sankey') valid = object(chart,['type','title','nodes','links']) && graph(nodes,links);
    if (!valid) throw Error('Invalid chart data');
    return chart;
  }

  function option(chart,theme) {
    const {type,data,title = ''} = chart;
    const colors = [theme.primary,theme.success,theme.warning,theme.danger,theme.foreground];
    const axis = {axisLine:{lineStyle:{color:theme.border}},axisLabel:{color:theme.muted_foreground},
      axisTick:{lineStyle:{color:theme.border}},splitLine:{show:true,lineStyle:{color:theme.border,type:'dashed'}}};
    const output = {animation:false,backgroundColor:theme.background,color:colors,
      textStyle:{color:theme.foreground,fontFamily:theme.font_family},
      title:{text:title,left:'center',textStyle:{color:theme.foreground,fontFamily:theme.font_family,fontSize:16}},series:[]};
    if (['line','bar','area','candlestick'].includes(type)) {
      output.xAxis = {...axis,type:'category',data:data.map(point => point.label)};
      output.yAxis = {...axis,type:'value'};output.grid = {left:64,right:24,top:title ? 64 : 24,bottom:64,containLabel:true};
      output.series = [{type:type === 'area' ? 'line' : type,
        data:data.map(point => type === 'candlestick' ? [point.open,point.close,point.low,point.high] : point.value),
        lineStyle:{width:3},...(type === 'area' ? {areaStyle:{opacity:.2}} : {}),
        ...(type === 'candlestick' ? {itemStyle:{color:theme.success,color0:theme.danger,borderColor:theme.success,borderColor0:theme.danger}} : {})}];
    } else if (type === 'pie') {
      output.series = [{type:'pie',radius:'65%',data:data.map(point => ({name:point.label,value:point.value})),
        label:{color:theme.foreground},labelLine:{lineStyle:{color:theme.muted_foreground}}}];
    } else if (type === 'radar') {
      output.radar = {indicator:data.map(point => ({name:point.label,max:Math.max(...data.map(point => point.value))*1.1})),
        axisName:{color:theme.foreground},axisLine:{lineStyle:{color:theme.border}},
        splitLine:{lineStyle:{color:theme.border}},splitArea:{show:false}};
      output.series = [{type:'radar',data:[{value:data.map(point => point.value)}],lineStyle:{width:3},areaStyle:{opacity:.15}}];
    } else {
      output.series = [{type:'sankey',data:chart.nodes.map(node => ({name:node.id})),links:chart.links,
        lineStyle:{color:'source',opacity:.45},label:{color:theme.foreground,
          formatter:params => chart.nodes.find(node => node.id === params.name)?.label ?? params.name}}];
    }
    return output;
  }

  function render(source,theme,width = 896) {
    const chart = parse(source);
    const instance = window.echarts.init(null,null,{renderer:'svg',ssr:true,width:Math.max(320,Math.min(960,width)),height:440});
    try {instance.setOption(option(chart,theme));return instance.renderToSVGString();}
    finally {instance.dispose();}
  }
  window.sailryCharts = {parse,option,render};
})();
