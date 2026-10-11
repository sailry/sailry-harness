(() => {
  'use strict';
  MathJax.loader.preLoaded('[mathjax-tex]/svg');
  let serial = 0;
  const surface = () => document.getElementById('surface');
  const color = value => /^#[0-9a-f]{6}$/i.test(value);
  const validate = request => {
    if (!['mermaid', 'inline_math', 'display_math'].includes(request.kind)
        || typeof request.source !== 'string' || request.source.length === 0
        || new TextEncoder().encode(request.source).length > 65536
        || ![request.foreground, request.background, request.accent].every(color)
        || !Number.isFinite(request.font_size) || request.font_size < 8 || request.font_size > 96) {
      throw new Error('Invalid visual request');
    }
  };
  // Native diagrams use the current text color and a transparent surface.
  const monochrome = (svg, foreground) => {
    const paint = (selector, fill, stroke) => {
      for (const element of svg.querySelectorAll(selector)) {
        element.style.setProperty('fill', fill, 'important');
        element.style.setProperty('stroke', stroke, 'important');
      }
    };
    for (const element of svg.querySelectorAll('*')) {
      element.style.setProperty('filter', 'none', 'important');
      element.style.setProperty('box-shadow', 'none', 'important');
      element.style.setProperty('text-shadow', 'none', 'important');
    }
    paint('.node rect,.node circle,.node ellipse,.node polygon,.node path,.node line,.node polyline,.cluster rect', 'none', foreground);
    paint('.flowchart-link,.edgePath path', 'none', foreground);
    paint('marker path,marker circle,marker polygon', foreground, foreground);
    paint('text,tspan', foreground, 'none');
    paint('.label rect,.edgeLabel rect,.labelBkg', 'none', 'none');
  };
  const finish = (svg, request, baseline) => {
    svg.style.color = request.foreground;
    svg.setAttribute('color', request.foreground);
    const bounds = svg.getBoundingClientRect();
    const width = bounds.width;
    const height = bounds.height;
    if (![width, height].every(value => value > 0 && value <= 4096)) {
      throw new Error('Rendered visual exceeds the supported size');
    }
    svg.setAttribute('width', String(width));
    svg.setAttribute('height', String(height));
    svg.removeAttribute('style');
    if (request.kind === 'mermaid') {
      svg.style.setProperty('background', 'transparent', 'important');
      svg.style.setProperty('filter', 'none', 'important');
      svg.style.setProperty('box-shadow', 'none', 'important');
      svg.style.setProperty('text-shadow', 'none', 'important');
    }
    let source = new XMLSerializer().serializeToString(svg).replace(/currentColor/gi, request.foreground);
    if (new TextEncoder().encode(source).length > 2097152) {
      throw new Error('Rendered visual exceeds the supported size');
    }
    return {source, width, height, baseline};
  };
  window.sailryRender = async request => {
    validate(request);
    const host = surface();
    host.replaceChildren();
    host.style.fontSize = `${request.font_size}px`;
    host.style.color = request.foreground;
    host.style.backgroundColor = request.background;
    try {
      if (request.kind === 'mermaid') {
        if (request.source.includes('%%{') || /^\s*---/.test(request.source)) {
          throw new Error('Diagram configuration directives are unsupported');
        }
        mermaid.initialize({startOnLoad:false,securityLevel:'strict',htmlLabels:false,maxTextSize:65536,maxEdges:500,
          theme:'base',themeVariables:{darkMode:request.dark,primaryColor:request.background,primaryTextColor:request.foreground,lineColor:request.foreground,primaryBorderColor:request.foreground,fontSize:`${request.font_size}px`},
          flowchart:{htmlLabels:false},secure:['securityLevel','startOnLoad','maxTextSize','maxEdges','htmlLabels']});
        const result = await mermaid.render(`diagram-${++serial}`, request.source);
        host.innerHTML = result.svg;
        const svg = host.querySelector('svg');
        if (!svg) throw new Error('Diagram did not produce SVG');
        monochrome(svg, request.foreground);
        return finish(svg, request, null);
      }
      await MathJax.startup.promise;
      const node = await MathJax.tex2svgPromise(request.source, {display:request.kind === 'display_math'});
      host.appendChild(node);
      const svg = node.querySelector('svg');
      if (!svg) throw new Error('Formula did not produce SVG');
      const bounds = svg.getBoundingClientRect();
      const vertical = svg.style.verticalAlign;
      const descent = /^-?[\d.]+ex$/.test(vertical) ? parseFloat(vertical) * request.font_size * 0.5 : 0;
      const baseline = request.kind === 'inline_math' ? Math.max(0, Math.min(bounds.height, bounds.height + descent)) : null;
      return finish(svg, request, baseline);
    } finally {
      host.replaceChildren();
      if (window.MathJax?.startup?.document) MathJax.startup.document.clear();
    }
  };
})();
