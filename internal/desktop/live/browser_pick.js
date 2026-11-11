// Invoked only by the browser tab's element picker with its own token. The
// page is untrusted and calls nothing of the app: picks wait in a queue that
// the picker reads through next().
token => {
  'use strict';
  window.__sailryElementSelection?.cancel();
  // A message stays within 32 KiB; keep room for the envelope.
  const LIMIT = 31 * 1024;
  const ATTRIBUTES = ['role','type','name','href','src','alt','title','placeholder','for','aria-label',
    'aria-describedby','aria-expanded','aria-selected','aria-checked','aria-disabled','aria-current',
    'data-testid','data-test','data-qa','data-cy'];
  const STYLES = ['color','background-color','font-size','font-family','display','position','padding','margin'];
  const encoder = new TextEncoder();
  const size = value => encoder.encode(value).length;
  const clean = value => String(value ?? '').replaceAll('\0', '');
  const clip = (value, limit) => {
    value = clean(value);
    if (size(value) <= limit) return value;
    let length = 0, result = '';
    for (const letter of value) {
      const bytes = size(letter);
      if (length + bytes > limit) break;
      result += letter; length += bytes;
    }
    return result;
  };
  const letters = (value, limit) => Array.from(clean(value)).slice(0, limit).join('');
  const layer = style => {
    const node = document.createElement('div');
    node.setAttribute('data-sailry-selection', '');
    Object.assign(node.style, {position:'fixed',pointerEvents:'none',zIndex:'2147483647',
      boxSizing:'border-box',display:'none',...style});
    document.documentElement.appendChild(node);
    return node;
  };
  const border = layer({border:'2px solid Highlight',background:'transparent'});
  const badge = layer({font:'12px/16px system-ui,sans-serif',color:'HighlightText',background:'Highlight',
    padding:'2px 6px',borderRadius:'4px',whiteSpace:'nowrap',maxWidth:'60vw',overflow:'hidden',textOverflow:'ellipsis'});
  const marks = new Map(), names = new WeakMap();
  let active = true, pending = 0, current = null, trail = [], fallback = null;
  const queue = [], readers = [];
  const send = value => {
    const message = JSON.stringify(value), reader = readers.shift();
    if (reader) reader(message); else queue.push(message);
  };
  const next = () => queue.length ? Promise.resolve(queue.shift()) : new Promise(resolve => readers.push(resolve));
  const block = event => { event.preventDefault(); event.stopImmediatePropagation(); };
  const origin = event => {
    const node = event.composedPath?.()[0] ?? event.target;
    return node instanceof Element ? node : null;
  };
  const up = element => element.parentElement ?? element.getRootNode?.()?.host ?? null;
  const down = element => element.firstElementChild ?? element.shadowRoot?.firstElementChild ?? null;
  const place = (node, rect) => Object.assign(node.style, {display:'block',left:`${rect.x}px`,top:`${rect.y}px`,
    width:`${rect.width}px`,height:`${rect.height}px`});

  const named = value => typeof value === 'string' && value.trim() ? clip(value.trim(), 128) : null;
  const react = (element, deadline) => {
    const key = Object.keys(element).find(name => name.startsWith('__reactFiber$') || name.startsWith('__reactInternalInstance$'));
    const host = key && element[key];
    // Only development builds retain owner and source metadata.
    if (!host || !['_debugOwner','_debugSource','_debugStack'].some(name => name in host)) return null;
    const title = type => !type || typeof type === 'string' ? null : named(type.displayName) ?? named(type.name)
      ?? (type.render ? named(type.render.displayName) ?? named(type.render.name) : null)
      ?? (type.type && type.type !== type ? title(type.type) : null);
    const stack = [];
    for (let fiber = host._debugOwner ?? host.return, steps = 0;
      fiber && steps < 200 && stack.length < 8 && performance.now() < deadline;
      fiber = fiber._debugOwner ?? fiber.return, steps++) {
      const name = title(fiber.type) ?? (fiber.type ? null : named(fiber.name));
      if (name && stack.at(-1) !== name) stack.push(name);
    }
    if (!stack.length) return null;
    const location = host._debugSource;
    const source = location?.fileName ? clip(`${location.fileName}:${location.lineNumber ?? 0}`, 512) : null;
    return {framework:'react',name:stack[0],source,stack};
  };
  const vue = element => {
    const stack = [];
    let source = null;
    for (let instance = element.__vueParentComponent, steps = 0; instance && steps < 50 && stack.length < 8;
      instance = instance.parent, steps++) {
      const type = instance.type ?? {};
      const file = typeof type.__file === 'string' ? type.__file : null;
      source ??= file ? clip(file, 512) : null;
      const name = named(type.name) ?? named(type.__name) ?? named(file?.split('/').pop().replace(/\.\w+$/, ''));
      if (name) stack.push(name);
    }
    return stack.length ? {framework:'vue',name:stack[0],source,stack} : null;
  };
  const svelte = element => {
    for (let node = element, steps = 0; node && steps < 20; node = node.parentElement, steps++) {
      const location = node.__svelte_meta?.loc;
      const name = typeof location?.file === 'string' ? named(location.file.split('/').pop().replace(/\.svelte$/, '')) : null;
      if (name) return {framework:'svelte',name,source:clip(`${location.file}:${location.line ?? 0}`, 512),stack:[name]};
    }
    return null;
  };
  const angular = element => {
    const ng = window.ng;
    if (typeof ng?.getComponent !== 'function') return null;
    for (let node = element, steps = 0; node && steps < 50; node = node.parentElement, steps++) {
      const instance = ng.getComponent(node) ?? ng.getOwningComponent?.(node);
      const name = named(instance?.constructor?.name?.replace(/^_+/, ''));
      if (name) return {framework:'angular',name,source:null,stack:[name]};
    }
    return null;
  };
  const component = element => {
    try {
      return react(element, performance.now() + 4) ?? vue(element) ?? svelte(element) ?? angular(element);
    } catch (_) {
      return null;
    }
  };

  const describe = element => {
    let name = names.get(element);
    if (name === undefined) {
      const classes = Array.from(element.classList ?? []).slice(0, 2).map(value => `.${value}`).join('');
      name = component(element)?.name ?? `${element.localName}${element.id ? `#${element.id}` : ''}${classes}`;
      names.set(element, name);
    }
    return name;
  };
  const show = element => {
    current = element;
    if (pending || !element) {
      border.style.display = badge.style.display = 'none';
      return;
    }
    const rect = element.getBoundingClientRect();
    place(border, rect);
    badge.textContent = `${describe(element)}  ${Math.round(rect.width)} × ${Math.round(rect.height)}`;
    Object.assign(badge.style, {display:'block',left:`${Math.max(0, Math.min(rect.x, innerWidth - 240))}px`,
      top:`${rect.y >= 22 ? rect.y - 22 : Math.max(0, Math.min(rect.y + rect.height + 4, innerHeight - 20))}px`});
  };
  const refresh = () => {
    for (const [element, mark] of marks) place(mark, element.getBoundingClientRect());
    show(current);
  };

  const unique = (scope, candidate, element) => {
    try { const nodes = scope.querySelectorAll(candidate); return nodes.length === 1 && nodes[0] === element; }
    catch (_) { return false; }
  };
  const scoped = (element, scope) => {
    if (element.id && element.id.length <= 256) {
      const candidate = `#${CSS.escape(element.id)}`;
      if (unique(scope, candidate, element)) return candidate;
    }
    for (const name of ['data-testid','data-test','data-qa','data-cy']) {
      const value = element.getAttribute(name);
      if (value && value.length <= 256) {
        const candidate = `[${name}="${CSS.escape(value)}"]`;
        if (unique(scope, candidate, element)) return candidate;
      }
    }
    const parts = [];
    for (let node = element; node; node = node.parentElement) {
      if (node.id && unique(scope, `#${CSS.escape(node.id)}`, node)) {
        parts.unshift(`#${CSS.escape(node.id)}`);
      } else {
        const siblings = Array.from(node.parentElement?.children ?? []).filter(sibling => sibling.localName === node.localName);
        parts.unshift(CSS.escape(node.localName) + (siblings.length > 1 ? `:nth-of-type(${siblings.indexOf(node) + 1})` : ''));
      }
      const candidate = parts.join(' > ');
      if (candidate.length > 2048) break;
      if (unique(scope, candidate, element)) return candidate;
    }
    // Deep or repeated structures keep their nearest segments instead of failing.
    let result = CSS.escape(element.localName);
    for (let start = parts.length - 1; start >= 0; start--) {
      const candidate = parts.slice(start).join(' > ');
      if (candidate.length > 2048) break;
      result = candidate;
    }
    return result;
  };
  const selector = element => {
    const root = element.getRootNode?.();
    const host = root && root !== document ? root.host : null;
    // Shadow content uses a piercing host prefix rather than a document selector.
    return clip(host ? `${selector(host)} >>> ${scoped(element, root)}` : scoped(element, document), 2048);
  };
  const path = element => {
    const parts = [];
    for (let node = element; node && parts.length < 12; node = up(node)) {
      const classes = Array.from(node.classList ?? []).slice(0, 2).map(value => `.${value}`).join('');
      parts.unshift(`${node.localName}${node.id ? `#${node.id}` : ''}${classes}`);
    }
    return clip(parts.join(' > '), 2048);
  };
  const accessible = element => {
    const label = element.getAttribute('aria-label');
    if (label?.trim()) return label;
    const ids = element.getAttribute('aria-labelledby');
    const labelled = ids?.split(/\s+/).map(id => document.getElementById(id)?.textContent ?? '').join(' ');
    if (labelled?.trim()) return labelled;
    const labels = Array.from(element.labels ?? []).map(node => node.textContent ?? '').join(' ');
    if (labels.trim()) return labels;
    return element.getAttribute('alt') ?? element.getAttribute('title') ?? '';
  };
  // Clones omit form values, scripts and styles before markup is captured.
  const markup = element => {
    try {
      const deep = (element.getElementsByTagName?.('*').length ?? 0) <= 500;
      const copy = element.cloneNode(deep);
      for (const node of [copy, ...(copy.querySelectorAll?.('*') ?? [])]) {
        if (['script','style','template'].includes(node.localName)) node.textContent = '';
        if (node.localName === 'textarea') node.textContent = '';
        node.removeAttribute?.('value');
      }
      return clip(copy.outerHTML, 2048);
    } catch (_) {
      return '';
    }
  };
  const styles = element => {
    try {
      const style = getComputedStyle(element);
      return Object.fromEntries(STYLES.map(name => [name, clip(style.getPropertyValue(name), 256)])
        .filter(([, value]) => value));
    } catch (_) {
      return {};
    }
  };
  const describeElement = element => {
    const rect = element.getBoundingClientRect();
    if (!(rect.width > 0 && rect.height > 0 && rect.x < innerWidth && rect.y < innerHeight
      && rect.x + rect.width > 0 && rect.y + rect.height > 0)) throw new Error('Element is outside the viewport');
    const text = clean(element.innerText).replace(/\s+/g, ' ').trim();
    const found = component(element);
    return {
      url:location.href,
      title:clip(document.title, 1024),
      selector:selector(element),
      path:path(element),
      tag:clip(element.localName.replace(/[^\w:-]/g, '_'), 64),
      id:clip(element.id, 256),
      classes:Array.from(element.classList ?? []).filter(value => value && !/\s/.test(value)).slice(0, 16)
        .map(value => clip(value, 128)),
      attributes:Object.fromEntries(ATTRIBUTES.filter(name => element.hasAttribute?.(name)).slice(0, 16)
        .map(name => [name, clip(element.getAttribute(name), 512)])),
      name:clip(accessible(element).replace(/\s+/g, ' ').trim(), 256),
      text:letters(text, 500),
      html:markup(element),
      styles:styles(element),
      component:found && {...found,stack:found.stack.slice(0, 8)},
      bounds:[rect.x,rect.y,rect.width,rect.height],
      viewport:[innerWidth,innerHeight],
      truncated:Array.from(text).length > 500,
    };
  };
  const fit = message => {
    const element = message.element;
    const reductions = [
      () => { element.html = ''; },
      () => { element.attributes = {}; },
      () => { element.text = letters(element.text, 100); },
      () => { element.path = ''; },
      () => { element.styles = {}; element.classes = []; },
      () => { if (element.component) element.component.stack = []; element.title = ''; },
    ];
    for (const reduce of reductions) {
      if (size(JSON.stringify(message)) <= LIMIT) break;
      reduce();
      element.truncated = true;
    }
    return message;
  };

  const stop = () => {
    if (!active) return;
    active = false;
    clearTimeout(fallback);
    document.removeEventListener('pointermove', hover, true);
    for (const name of ['pointerdown','pointerup','mousedown','mouseup']) document.removeEventListener(name, block, true);
    document.removeEventListener('click', pick, true);
    document.removeEventListener('keydown', key, true);
    removeEventListener('scroll', refresh, true);
    removeEventListener('resize', refresh);
    border.remove(); badge.remove();
    for (const mark of marks.values()) mark.remove();
    marks.clear();
    // The stopped selection stays for next(): its last messages are still read.
  };
  const resume = value => {
    if (value !== token || !active || !pending) return;
    if (--pending === 0) { clearTimeout(fallback); show(current); }
  };
  const commit = (element, review) => {
    const elements = review ? [element] : [...marks.keys()];
    if (!elements.includes(element)) elements.push(element);
    for (const mark of marks.values()) mark.remove();
    marks.clear();
    const messages = [];
    for (const item of elements) {
      try { messages.push(fit({kind:'element',token,element:describeElement(item),review})); }
      catch (_) { /* An element that cannot be described is skipped. */ }
    }
    if (!messages.length) return;
    pending = messages.length;
    show(current);
    if (review) stop();
    else fallback = setTimeout(() => { pending = 0; show(current); }, 10000);
    // Overlays must be gone before the native viewport snapshot begins.
    requestAnimationFrame(() => requestAnimationFrame(() => { for (const message of messages) send(message); }));
  };
  const toggle = element => {
    const mark = marks.get(element);
    if (mark) { mark.remove(); marks.delete(element); return; }
    if (marks.size >= 8) return;
    const node = layer({border:'2px dashed Highlight',background:'transparent'});
    place(node, element.getBoundingClientRect());
    marks.set(element, node);
  };
  const hover = event => {
    const element = origin(event);
    if (element && element !== current) { trail = []; show(element); }
  };
  const pick = event => {
    if (event.button !== 0) return;
    block(event);
    const element = origin(event);
    if (!element || pending) return;
    if (event.shiftKey) toggle(element);
    else commit(element, event.altKey);
  };
  const key = event => {
    if (event.key === 'Escape') {
      block(event); stop(); send({kind:'cancelled',token});
      return;
    }
    if (!current || pending) return;
    if (event.key === 'ArrowUp') {
      block(event);
      const parent = up(current);
      if (parent) { trail.push(current); show(parent); }
    } else if (event.key === 'ArrowDown') {
      block(event);
      const child = trail.pop() ?? down(current);
      if (child) show(child);
    } else if (event.key === 'Enter') {
      block(event);
      if (event.shiftKey) toggle(current);
      else commit(current, event.altKey);
    }
  };
  document.addEventListener('pointermove', hover, true);
  for (const name of ['pointerdown','pointerup','mousedown','mouseup']) document.addEventListener(name, block, true);
  document.addEventListener('click', pick, true);
  document.addEventListener('keydown', key, true);
  addEventListener('scroll', refresh, true);
  addEventListener('resize', refresh);
  window.__sailryElementSelection = {token,cancel:stop,resume,next};
  return true;
}
