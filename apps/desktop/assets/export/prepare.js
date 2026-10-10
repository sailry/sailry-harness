MathJax.loader.preLoaded('[mathjax-tex]/svg');
window.sailryExportReady = new Promise((resolve, reject) => {
  window.addEventListener('DOMContentLoaded', async () => {
    try {
      await MathJax.startup.promise;
      for (const element of document.querySelectorAll('code.language-chart')) {
        if(element.textContent.length > 65536) throw Error("Chart exceeds the supported size");
        const chart = JSON.parse(element.textContent);
        const {type, data, title = ''} = chart;
        const option = {animation:false, title:{text:title,left:'center'}, color:['#2563eb','#0891b2','#d97706','#7c3aed','#e11d48'], series:[]};
        if (['line','bar','area','candlestick'].includes(type)) {
          option.xAxis = {type:'category',data:data.map(point => point.label)};
          option.yAxis = {type:'value'};
          option.grid = {left:64,right:32,top:64,bottom:64};
          option.series = [{type:type==='area'?'line':type, data:data.map(point => type==='candlestick'?[point.open,point.close,point.low,point.high]:point.value), ...(type==='area'?{areaStyle:{}}:{})}];
        } else if(type==='pie') {
          option.series = [{type:'pie',radius:'65%',data:data.map(point=>({name:point.label,value:point.value}))}];
        } else if(type==='radar') {
          option.radar = {indicator:data.map(point=>({name:point.label,max:Math.max(...data.map(point=>point.value))*1.1}))};
          option.series = [{type:'radar',data:[{value:data.map(point=>point.value)}]}];
        } else if(type==='sankey') {
          option.series = [{type:'sankey',data:chart.nodes.map(node=>({name:node.id})),links:chart.links,label:{formatter:params=>chart.nodes.find(node=>node.id===params.name)?.label||params.name}}];
        } else { throw Error('Unsupported chart type'); }
        const canvas = document.createElement('div');
        const instance = echarts.init(null,null,{renderer:'svg',ssr:true,width:896,height:440});
        try { instance.setOption(option); canvas.innerHTML=instance.renderToSVGString(); } finally { instance.dispose(); }
        canvas.querySelector('svg').classList.add('export-visual');
        element.parentElement.replaceWith(canvas);
      }
      mermaid.initialize({startOnLoad:false,securityLevel:'strict',htmlLabels:false,maxTextSize:65536,maxEdges:500,theme:'default',flowchart:{htmlLabels:false}});
      let serial=0;
      for (const element of document.querySelectorAll('code.language-mermaid')) {
        const source=element.textContent;
        if(source.includes('%%{') || /^\s*---/.test(source)) throw Error('Diagram configuration is unsupported');
        const result=await mermaid.render(`export-diagram-${serial++}`,source);
        const canvas=document.createElement('div');canvas.innerHTML=result.svg;
        element.parentElement.replaceWith(canvas);
      }
      await MathJax.typesetPromise();
      resolve();
    } catch(error) { reject(error); }
  }, {once:true});
});
