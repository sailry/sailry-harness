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
    tag, tagName:tag, children, parent: null,
    get attributes() { return [...values].map(([name,value]) => ({name,value})); },
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
    replaceWith(next) {
      const index = this.parent.children.indexOf(this);
      this.parent.children[index] = next; next.parent = this.parent;
    },
    closest(selector) {
      for (let node=this; node; node=node.parent) {
        if (selector.split(',').some(part => matches(node,part.trim()))) return node;
      }
      return null;
    },
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
  const diamond = element('polygon', {id:'diamond', points: '0,10 10,0 20,10 10,20', transform:'translate(2,3)'});
  const triangle = element('path', {id:'triangle', class:'outline', d:'M0 20 L10 0 L20 20 Z'});
  const cubic = element('path', {id:'cubic', d:'M0 20 C3 14 7 6 10 0 C13 6 17 14 20 20 L0 20 Z'});
  const curved = element('path', {id:'curved', d:'M0 20 C0 0 20 0 20 20 Z'});
  const open = element('path', {id:'open', d:'M0 20 L10 0 L20 20'});
  const degenerate = element('polygon', {id:'degenerate', points:'0,0 0,0 10,10'});
  const invalid = element('polygon', {id:'invalid', points:'0,0 10,Infinity 20,0'});
  const circle = element('circle', {r: '10'});
  const ellipse = element('ellipse', {rx: '10', ry: '20'});
  const edge = element('path', {class: 'flowchart-link', d:'M0 0 L10 0 L10 10 Z', 'marker-end': 'url(#arrow)', stroke: 'url(#gradient)'});
  const arrow = element('path', {d:'M0 0 L10 0 L5 10 Z'});
  const text = element('text', {fill: '#ff0000'}, [element('tspan')]);
  const label = element('rect', {class: 'labelBkg'});
  const svg = element('svg', {xmlns: 'http://www.w3.org/2000/svg', filter: 'url(#glow)'}, [
    element('defs', {}, [element('linearGradient', {id: 'gradient'}), element('filter', {id: 'glow'}), element('marker', {id: 'arrow'}, [arrow])]),
    ...[rect, rounded, diamond, triangle, cubic, curved, open, degenerate, invalid, circle, ellipse].map(shape => element('g', {class: 'node'}, [shape])),
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
  return {svg, rect, rounded, diamond, triangle, cubic, curved, open, degenerate, invalid, circle, ellipse, edge, arrow, text, label};
}

function setup(source, create = diagram) {
  const host = element('main');
  let current;
  Object.defineProperty(host, 'innerHTML', {set() { current = create(); host.replaceChildren(current.svg); }});
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
    document: {
      getElementById(id) { assert.equal(id, 'surface'); return host; },
      createElementNS(namespace,tag) { assert.equal(namespace,'http://www.w3.org/2000/svg'); return element(tag); },
    },
    XMLSerializer: class { serializeToString(svg) { return svg.serialize(); } },
  });
  return {render: window.sailryRender, host, initialized, rendered, formulas,
    current: () => current, cleared: () => cleared};
}

const request = (dark = false) => ({kind: 'mermaid', source: 'flowchart LR\nA-->B', dark,
  foreground: dark ? '#ededed' : '#242424', background: dark ? '#181818' : '#ffffff', accent: '#775500', font_size: 16});

for (const [name, path] of scripts) {
  const source = readFileSync(path, 'utf8');
  if (name === 'desktop') {
    test('desktop preserves intrinsic diagram geometry', async () => {
      for (const [diagramSource, box, width, height] of [
        ['flowchart LR\nA[Long label]-->B[Another label]', '0 0 1200 220', 1200, 220],
        ['flowchart TD\nA-->B', '-8 -12 160 600', 160, 600],
      ]) {
        const view = setup(source, () => {
          const value = diagram(); value.svg.setAttribute('viewBox', box); return value;
        });
        const result = await view.render({...request(true), source: diagramSource});
        assert.equal(view.rendered.at(-1).source, diagramSource);
        assert.equal(result.width, width);
        assert.equal(result.height, height);
        assert(result.source.includes(`viewBox="${box}"`));
        assert(result.source.includes(`width="${width}"`));
        assert(result.source.includes(`height="${height}"`));
      }
      for (const box of ['0 0 NaN 220', '0 0 -1 220', '0 0 0 220', '0 0 5000 220', '0 0 1200']) {
        const view = setup(source, () => {
          const value = diagram(); value.svg.setAttribute('viewBox', box); return value;
        });
        await assert.rejects(view.render(request()), /Rendered visual exceeds the supported size/);
      }
    });
  }
  test(`${name} serializes solid themed diagram paint`, async () => {
    const view = setup(source);
    for (const dark of [false, true]) {
      const input = request(dark), result = await view.render(input);
      const {svg, rect, rounded, circle, ellipse, edge, arrow, text, label} = view.current();
      const diamond = svg.querySelectorAll('*').find(node => node.getAttribute('id') === 'diamond');
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
      assert.equal(diamond.tag, name === 'desktop' ? 'path' : 'polygon');
      assert.equal(diamond.getAttribute('transform'), 'translate(2,3)');
      if (name === 'desktop') {
        assert.equal(diamond.getAttribute('points'), null);
        assert.equal((diamond.getAttribute('d').match(/Q/g) ?? []).length, 4);
      } else assert.equal(diamond.getAttribute('points'), '0,10 10,0 20,10 10,20');
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

  test(`${name} preserves unsupported outlines and rounds straight nodes`, async () => {
    const view = setup(source);
    await view.render(request());
    const {triangle,cubic,curved,open,degenerate,invalid,edge,arrow} = view.current();
    if (name === 'desktop') {
      for (const shape of [triangle,cubic]) {
        assert.equal((shape.getAttribute('d').match(/Q/g) ?? []).length, 3);
        assert(shape.getAttribute('d').endsWith('Z'));
      }
      assert.equal(triangle.getAttribute('d'),cubic.getAttribute('d'));
      assert.equal(triangle.getAttribute('class'),'outline');
    } else {
      assert.equal(triangle.getAttribute('d'),'M0 20 L10 0 L20 20 Z');
      assert.equal(cubic.getAttribute('d'),'M0 20 C3 14 7 6 10 0 C13 6 17 14 20 20 L0 20 Z');
    }
    assert.equal(curved.getAttribute('d'),'M0 20 C0 0 20 0 20 20 Z');
    assert.equal(open.getAttribute('d'),'M0 20 L10 0 L20 20');
    assert.equal(degenerate.getAttribute('points'),'0,0 0,0 10,10');
    assert.equal(invalid.getAttribute('points'),'0,0 10,Infinity 20,0');
    assert.equal(edge.getAttribute('d'),'M0 0 L10 0 L10 10 Z');
    assert.equal(arrow.getAttribute('d'),'M0 0 L10 0 L5 10 Z');
  });

  test(`${name} rounds implicit and repeated closed outlines`, async () => {
    for (const path of [
      'M0 0 C3 0 7 0 10 0 C9 -2 7 -6 5 -10 C4 -8 1 -2 0 0',
      'M0 0 C3 0 7 0 10 0 M0 0 C2 0 8 0 10 0 '
        + 'M10 0 C9 -2 7 -6 5 -10 M10 0 C8 -4 6 -8 5 -10 '
        + 'M5 -10 C4 -8 1 -2 0 0 M5 -10 C3 -6 2 -4 0 0',
    ]) {
      const view = setup(source, () => {
        const value = diagram(); value.triangle.setAttribute('d',path); return value;
      });
      await view.render(request());
      const actual = view.current().triangle.getAttribute('d');
      if (name === 'desktop') {
        assert.equal((actual.match(/Q/g) ?? []).length,3);
        assert(actual.endsWith('Z'));
        assert.equal(view.current().triangle.getAttribute('class'),'outline');
      } else assert.equal(actual,path);
    }
  });

  test(`${name} leaves bounded invalid geometry unchanged`, async () => {
    for (const path of [
      'M0 0 L10 10 L20 20 Z',
      'M0 0 L10 0 L10 10 Z M20 20 L30 20 L30 30 Z',
      'M0 0 L10 0 M10 0 L5 -10',
      'M0 0 L10 0 L5 -10 L0 0 M10 0 L15 5',
      'M0 0 L10 0 M5 -10 L0 0 M10 0 L5 -10',
      'M0 0 L10 0 M20 20 L30 20 M30 20 L20 20',
      'M0 0 L0 0 L10 0 L5 -10 Z',
      'M0 0 A10 10 0 0 0 20 20 Z',
      'M0 0 L1e308 0 L0 20 Z',
      'M0 0 ' + 'L1 1 '.repeat(600) + 'Z',
    ]) {
      const view = setup(source, () => {
        const value = diagram(); value.triangle.setAttribute('d',path); return value;
      });
      await view.render(request());
      assert.equal(view.current().triangle.getAttribute('d'),path);
    }
  });
}
