import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';

const scripts = [
  ['desktop', new URL('../../../assets/visuals/render.js', import.meta.url)],
  ['mobile', new URL('../../../../../apps/mobile/assets/visuals/render.js', import.meta.url)],
];

function element(tag, attributes = {}, children = []) {
  const values = new Map(Object.entries(attributes));
  const properties = new Map();
  const node = {
    tag, children, parent: null,
    style: {
      setProperty(name, value, priority) { properties.set(name, {value, priority}); },
      getPropertyValue(name) { return properties.get(name)?.value ?? ''; },
      getPropertyPriority(name) { return properties.get(name)?.priority ?? ''; },
    },
    setAttribute(name, value) { values.set(name, String(value)); },
    getAttribute(name) { return values.get(name) ?? null; },
    removeAttribute(name) { values.delete(name); if (name === 'style') properties.clear(); },
    replaceChildren(...next) { this.children = next; for (const child of next) child.parent = this; },
    appendChild(child) { child.parent = this; this.children.push(child); },
    getBoundingClientRect() { return {width: 320, height: 96}; },
    querySelectorAll(selector) {
      const descendants = this.children.flatMap(child => [child, ...child.querySelectorAll('*')]);
      return descendants.filter(child => selector.split(',').some(part => matches(child, part.trim())));
    },
    querySelector(selector) { return this.querySelectorAll(selector)[0] ?? null; },
    serialize() {
      const attributes = [...values].map(([name, value]) => `${name}="${value}"`);
      if (properties.size) {
        attributes.push(`style="${[...properties].map(([name, {value, priority}]) => `${name}:${value}${priority ? ` !${priority}` : ''}`).join(';')}"`);
      }
      return `<${tag}${attributes.length ? ` ${attributes.join(' ')}` : ''}>${this.children.map(child => child.serialize()).join('')}</${tag}>`;
    },
  };
  for (const child of children) child.parent = node;
  return node;
}

function matches(node, selector) {
  const parts = selector.split(/\s+/);
  const target = parts.pop();
  const single = (node, part) => part === '*' || (part.startsWith('.')
    ? (node.getAttribute('class') ?? '').split(/\s+/).includes(part.slice(1))
    : node.tag === part);
  if (!single(node, target)) return false;
  for (const part of parts.reverse()) {
    let ancestor = node.parent;
    while (ancestor && !single(ancestor, part)) ancestor = ancestor.parent;
    if (!ancestor) return false;
    node = ancestor;
  }
  return true;
}

function diagram() {
  const rect = element('rect', {fill: 'url(#gradient)', stroke: 'url(#gradient)', rx: '0', ry: '0'});
  const rounded = element('rect', {rx: '20', ry: '20'});
  const diamond = element('polygon', {points: '0,10 10,0 20,10 10,20'});
  const circle = element('circle', {r: '10'});
  const ellipse = element('ellipse', {rx: '10', ry: '20'});
  const edge = element('path', {class: 'flowchart-link', 'marker-end': 'url(#arrow)', stroke: 'url(#gradient)'});
  const arrow = element('path');
  const text = element('text', {fill: '#ff0000'}, [element('tspan')]);
  const label = element('rect', {class: 'labelBkg'});
  const svg = element('svg', {xmlns: 'http://www.w3.org/2000/svg', filter: 'url(#glow)'}, [
    element('defs', {}, [element('linearGradient', {id: 'gradient'}), element('filter', {id: 'glow'}), element('marker', {id: 'arrow'}, [arrow])]),
    ...[rect, rounded, diamond, circle, ellipse].map(shape => element('g', {class: 'node'}, [shape])),
    edge, element('g', {class: 'label'}, [label, text]),
  ]);
  for (const node of [svg, ...svg.querySelectorAll('*')]) {
    node.setAttribute('filter', 'url(#glow)');
    node.style.setProperty('filter', 'url(#glow)', 'important');
    node.style.setProperty('box-shadow', '0 0 8px red', 'important');
    node.style.setProperty('text-shadow', '0 0 8px red', 'important');
  }
  rect.style.setProperty('stroke', 'url(#gradient)', 'important');
  rect.style.setProperty('fill', 'url(#gradient)', 'important');
  return {svg, rect, rounded, diamond, circle, ellipse, edge, arrow, text, label};
}

function setup(source) {
  const host = element('main');
  let current;
  Object.defineProperty(host, 'innerHTML', {set() { current = diagram(); host.replaceChildren(current.svg); }});
  const initialized = [], rendered = [], formulas = [];
  let cleared = 0;
  const MathJax = {
    loader: {preLoaded() {}},
    startup: {promise: Promise.resolve(), document: {clear() { cleared++; }}},
    async tex2svgPromise(source, options) {
      formulas.push({source, options});
      const svg = element('svg', {xmlns: 'http://www.w3.org/2000/svg'}, [element('path', {fill: 'currentColor'})]);
      svg.style.verticalAlign = '-1ex';
      return element('mjx-container', {}, [svg]);
    },
  };
  const mermaid = {
    initialize(config) { initialized.push(config); },
    async render(id, source) { rendered.push({id, source}); return {svg: '<svg/>'}; },
  };
  const window = {MathJax};
  vm.runInNewContext(source, {
    window, MathJax, mermaid, TextEncoder,
    document: {getElementById(id) { assert.equal(id, 'surface'); return host; }},
    XMLSerializer: class { serializeToString(svg) { return svg.serialize(); } },
  });
  return {render: window.sailryRender, host, initialized, rendered, formulas,
    current: () => current, cleared: () => cleared};
}

const request = (dark = false) => ({kind: 'mermaid', source: 'flowchart LR\nA-->B', dark,
  foreground: dark ? '#ededed' : '#242424', background: dark ? '#181818' : '#ffffff', accent: '#775500', font_size: 16});

for (const [name, path] of scripts) {
  const source = readFileSync(path, 'utf8');
  test(`${name} serializes solid themed diagram paint`, async () => {
    const view = setup(source);
    for (const dark of [false, true]) {
      const input = request(dark), result = await view.render(input);
      const {svg, rect, rounded, diamond, circle, ellipse, edge, arrow, text, label} = view.current();
      const config = view.initialized.at(-1);
      const outline = name === 'desktop' ? input.accent : input.foreground;
      assert.equal(config.themeVariables.primaryTextColor, input.foreground);
      assert.equal(config.themeVariables.primaryBorderColor, outline);
      assert.equal(config.themeVariables.lineColor, outline);
      assert.equal(config.themeVariables.darkMode, dark);
      for (const shape of [rect, rounded, diamond, circle, ellipse, edge]) {
        assert.equal(shape.getAttribute('fill'), 'none');
        assert.equal(shape.getAttribute('stroke'), outline);
        assert.equal(shape.style.getPropertyValue('stroke'), outline);
        assert.equal(shape.style.getPropertyPriority('stroke'), 'important');
      }
      for (const node of [svg, ...svg.querySelectorAll('*')]) {
        assert.equal(node.getAttribute('filter'), 'none');
        for (const property of ['filter', 'box-shadow', 'text-shadow']) {
          assert.equal(node.style.getPropertyValue(property), 'none');
        }
      }
      assert.equal(svg.style.getPropertyValue('background'), 'transparent');
      assert.equal(arrow.getAttribute('fill'), outline);
      for (const node of [text, ...text.children]) {
        assert.equal(node.getAttribute('fill'), input.foreground);
        assert.equal(node.getAttribute('stroke'), 'none');
      }
      assert.equal(label.getAttribute('fill'), 'none');
      assert.equal(label.getAttribute('stroke'), 'none');
      assert(Number(rect.getAttribute('rx')) > 0);
      assert.equal(rect.getAttribute('rx'), rect.getAttribute('ry'));
      assert.equal(rounded.getAttribute('rx'), '20');
      assert.equal(rounded.getAttribute('ry'), '20');
      assert.equal(diamond.getAttribute('points'), '0,10 10,0 20,10 10,20');
      assert.equal(diamond.getAttribute('rx'), null);
      assert.equal(circle.getAttribute('r'), '10');
      assert.equal(circle.getAttribute('rx'), null);
      assert.equal(ellipse.getAttribute('rx'), '10');
      assert.equal(ellipse.getAttribute('ry'), '20');
      assert.equal(edge.getAttribute('marker-end'), 'url(#arrow)');
      assert(result.source.includes(`stroke="${outline}"`));
      assert(!result.source.includes('stroke="url('));
      assert(!result.source.includes('fill="url('));
      assert(!result.source.includes('filter="url('));
      assert.equal(result.baseline, null);
      assert.equal(view.host.children.length, 0);
    }
    assert.notEqual(view.rendered[0].id, view.rendered[1].id);
    assert.equal(view.cleared(), 2);
  });

  test(`${name} retains formula colors and baseline`, async () => {
    const view = setup(source);
    for (const kind of ['inline_math', 'display_math']) {
      const input = {...request(), kind, source: 'x^2'}, result = await view.render(input);
      assert.equal(view.formulas.at(-1).options.display, kind === 'display_math');
      assert(result.source.includes(`fill="${input.foreground}"`));
      assert(!result.source.includes('currentColor'));
      assert(!result.source.includes('filter='));
      assert(!result.source.includes('rx='));
      assert.equal(result.baseline, kind === 'inline_math' ? 88 : null);
      assert.equal(view.host.children.length, 0);
    }
    assert.equal(view.initialized.length, 0);
    assert.equal(view.rendered.length, 0);
    assert.equal(view.cleared(), 2);
  });
}
