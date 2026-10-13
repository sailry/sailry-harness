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
  const numbers = value => {
    if (typeof value !== 'string' || value.length > 16384) return null;
    const tokens = value.match(/[-+]?(?:\d*\.\d+|\d+\.?\d*)(?:e[-+]?\d+)?/gi) ?? [];
    if (tokens.length > 512 || value.replace(/[-+]?(?:\d*\.\d+|\d+\.?\d*)(?:e[-+]?\d+)?/gi, '').replace(/[\s,]/g, '')) return null;
    const result = tokens.map(Number);
    return result.every(value => Number.isFinite(value) && Math.abs(value) <= 1000000) ? result : null;
  };
  const straight = (start, end, controls) => {
    const dx = end[0] - start[0], dy = end[1] - start[1], length = dx * dx + dy * dy;
    if (!(length > 0)) return false;
    let previous = 0;
    for (const point of controls) {
      const x = point[0] - start[0], y = point[1] - start[1];
      const position = (x * dx + y * dy) / length;
      if (Math.abs(x * dy - y * dx) > 0.000001 * length
          || position < previous || position > 1) return false;
      previous = position;
    }
    return true;
  };
  // RoughJS may close collinear cubic sides implicitly and repeat edge subpaths.
  const outline = value => {
    if (typeof value !== 'string' || value.length > 16384) return null;
    const tokens = value.match(/[a-z]|[-+]?(?:\d*\.\d+|\d+\.?\d*)(?:e[-+]?\d+)?/gi) ?? [];
    if (tokens.length > 2048 || value.replace(/[a-z]|[-+]?(?:\d*\.\d+|\d+\.?\d*)(?:e[-+]?\d+)?/gi, '').replace(/[\s,]/g, '')) return null;
    const edges = [];
    const same = (left, right) => left[0] === right[0] && left[1] === right[1];
    let current = [0, 0], start, index = 0, count = 0, command;
    while (index < tokens.length) {
      if (/^[a-z]$/i.test(tokens[index])) command = tokens[index++];
      if (!command) return null;
      const kind = command.toUpperCase(), relative = command !== kind;
      if (kind === 'Z') {
        if (!start) return null;
        if (!same(current, start)) edges.push([current, start]);
        current = start; command = null;
        continue;
      }
      const size = {M:2, L:2, H:1, V:1, C:6}[kind];
      if (!size || (!start && kind !== 'M')
          || index + size > tokens.length) return null;
      const values = tokens.slice(index, index += size).map(Number);
      if (!values.every(value => Number.isFinite(value) && Math.abs(value) <= 1000000)) return null;
      const point = (x, y) => relative ? [x + current[0], y + current[1]] : [x, y];
      let next;
      if (kind === 'H') next = [values[0] + (relative ? current[0] : 0), current[1]];
      else if (kind === 'V') next = [current[0], values[0] + (relative ? current[1] : 0)];
      else next = point(values.at(-2), values.at(-1));
      if (!next.every(value => Math.abs(value) <= 1000000)) return null;
      if (kind === 'C' && !straight(current, next, [point(values[0], values[1]), point(values[2], values[3])])) return null;
      if (++count > 256) return null;
      if (kind === 'M') start = next;
      else {
        if (same(current, next)) return null;
        edges.push([current, next]);
      }
      current = next;
      if (kind === 'M') command = relative ? 'l' : 'L';
    }
    const points = [], seen = new Set();
    for (const [from, to] of edges) {
      const key = [from.join(','), to.join(',')].sort().join(';');
      if (seen.has(key)) continue;
      seen.add(key);
      if (points.length === 0) points.push(from);
      else if (!same(points.at(-1), from)) return null;
      points.push(to);
    }
    if (points.length < 4 || !same(points[0], points.at(-1))) return null;
    const vertices = points.slice(0, -1).map(point => point.join(','));
    return new Set(vertices).size === vertices.length ? points : null;
  };
  const rounded = (points, radius) => {
    if (!points || points.length < 3) return null;
    const same = (left, right) => left[0] === right[0] && left[1] === right[1];
    if (same(points[0], points.at(-1))) points = points.slice(0, -1);
    if (points.length < 3) return null;
    const area = points.reduce((sum, point, index) => {
      const next = points[(index + 1) % points.length];
      return sum + point[0] * next[1] - next[0] * point[1];
    }, 0);
    if (Math.abs(area) < 0.000001) return null;
    const corners = [];
    for (let index = 0; index < points.length; index++) {
      const point = points[index], before = points[(index + points.length - 1) % points.length];
      const after = points[(index + 1) % points.length];
      const incoming = Math.hypot(before[0] - point[0], before[1] - point[1]);
      const outgoing = Math.hypot(after[0] - point[0], after[1] - point[1]);
      if (!(incoming > 0 && outgoing > 0)) return null;
      const distance = Math.min(radius, incoming / 3, outgoing / 3);
      const offset = (other, length) => point.map((value, axis) => value + (other[axis] - value) * distance / length);
      corners.push({point, start:offset(before, incoming), end:offset(after, outgoing)});
    }
    const pair = point => point.join(',');
    return 'M' + pair(corners[0].start) + corners.map((corner, index) =>
      (index ? 'L' + pair(corner.start) : '') + 'Q' + pair(corner.point) + ' ' + pair(corner.end)).join('') + 'Z';
  };
  const roundNodes = (svg, radius) => {
    for (const element of svg.querySelectorAll('.node polygon,.node path')) {
      if (element.closest('defs,marker,.label,.edgeLabel')) continue;
      let points;
      if (element.tagName.toLowerCase() === 'polygon') {
        const values = numbers(element.getAttribute('points'));
        points = values && values.length % 2 === 0
          ? Array.from({length:values.length / 2}, (_, index) => values.slice(index * 2, index * 2 + 2)) : null;
      } else points = outline(element.getAttribute('d'));
      const path = rounded(points, radius);
      if (!path) continue;
      if (element.tagName.toLowerCase() === 'path') element.setAttribute('d', path);
      else {
        const replacement = document.createElementNS('http://www.w3.org/2000/svg', 'path');
        for (const attribute of element.attributes) {
          if (attribute.name !== 'points') replacement.setAttribute(attribute.name, attribute.value);
        }
        replacement.setAttribute('d', path);
        element.replaceWith(replacement);
      }
    }
  };
  // Diagram outlines use their semantic accent separately from label text.
  const monochrome = (svg, foreground, outline, fontSize) => {
    roundNodes(svg, fontSize * 0.35);
    const paint = (selector, fill, stroke) => {
      for (const element of svg.querySelectorAll(selector)) {
        element.setAttribute('fill', fill);
        element.setAttribute('stroke', stroke);
        element.style.setProperty('fill', fill, 'important');
        element.style.setProperty('stroke', stroke, 'important');
      }
    };
    for (const element of svg.querySelectorAll('*')) {
      element.setAttribute('filter', 'none');
      element.style.setProperty('filter', 'none', 'important');
      element.style.setProperty('box-shadow', 'none', 'important');
      element.style.setProperty('text-shadow', 'none', 'important');
    }
    paint('.node rect,.node circle,.node ellipse,.node polygon,.node path,.node line,.node polyline,.cluster rect', 'none', outline);
    paint('.flowchart-link,.edgePath path', 'none', outline);
    paint('marker path,marker circle,marker polygon', outline, outline);
    paint('text,tspan', foreground, 'none');
    paint('.label rect,.edgeLabel rect,.labelBkg', 'none', 'none');
    for (const rect of svg.querySelectorAll('.node rect')) {
      if (!(parseFloat(rect.getAttribute('rx')) > 0) && !(parseFloat(rect.getAttribute('ry')) > 0)) {
        rect.setAttribute('rx', String(fontSize * 0.35));
        rect.setAttribute('ry', String(fontSize * 0.35));
      }
    }
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
      svg.setAttribute('filter', 'none');
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
          theme:'base',themeVariables:{darkMode:request.dark,primaryColor:request.background,primaryTextColor:request.foreground,lineColor:request.accent,primaryBorderColor:request.accent,fontSize:`${request.font_size}px`},
          flowchart:{htmlLabels:false},secure:['securityLevel','startOnLoad','maxTextSize','maxEdges','htmlLabels']});
        const result = await mermaid.render(`diagram-${++serial}`, request.source);
        host.innerHTML = result.svg;
        const svg = host.querySelector('svg');
        if (!svg) throw new Error('Diagram did not produce SVG');
        monochrome(svg, request.foreground, request.accent, request.font_size);
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
