token => {
  'use strict';
  window.__sailryElementSelection?.cancel();
  const border = document.createElement('div');
  border.setAttribute('data-sailry-selection', '');
  Object.assign(border.style, {position:'fixed',pointerEvents:'none',zIndex:'2147483647',
    border:'2px solid Highlight',background:'transparent',boxSizing:'border-box',display:'none'});
  document.documentElement.appendChild(border);
  let active = true;
  const send = value => window.ipc.postMessage(JSON.stringify(value));
  const block = event => { event.preventDefault(); event.stopImmediatePropagation(); };
  const cancel = () => {
    if (!active) return;
    active = false;
    document.removeEventListener('pointermove', hover, true);
    for (const name of ['pointerdown','pointerup','mousedown','mouseup']) document.removeEventListener(name, block, true);
    document.removeEventListener('click', pick, true);
    document.removeEventListener('keydown', key, true);
    border.remove();
    if (window.__sailryElementSelection?.token === token) delete window.__sailryElementSelection;
  };
  const hover = event => {
    const rect = event.target?.getBoundingClientRect?.();
    if (!rect) return;
    Object.assign(border.style, {display:'block',left:`${rect.x}px`,top:`${rect.y}px`,width:`${rect.width}px`,height:`${rect.height}px`});
  };
  const unique = (selector, element) => {
    try { const nodes = document.querySelectorAll(selector); return nodes.length === 1 && nodes[0] === element; }
    catch (_) { return false; }
  };
  const selector = element => {
    if (element.getRootNode() !== document) throw new Error('Unsupported element root');
    if (element.id) {
      const candidate = `#${CSS.escape(element.id)}`;
      if (unique(candidate, element)) return candidate;
    }
    for (const name of ['data-testid','data-test','data-qa']) {
      const value = element.getAttribute(name);
      if (value && value.length <= 256) {
        const candidate = `[${name}="${CSS.escape(value)}"]`;
        if (unique(candidate, element)) return candidate;
      }
    }
    const parts = [];
    for (let node = element, depth = 0; node && depth < 12; node = node.parentElement, depth++) {
      let part = CSS.escape(node.localName);
      if (node.id && unique(`#${CSS.escape(node.id)}`, node)) {
        parts.unshift(`#${CSS.escape(node.id)}`);
      } else {
        const siblings = Array.from(node.parentElement?.children ?? []).filter(sibling => sibling.localName === node.localName);
        if (siblings.length > 1) part += `:nth-of-type(${siblings.indexOf(node) + 1})`;
        parts.unshift(part);
      }
      const candidate = parts.join(' > ');
      if (candidate.length <= 2048 && unique(candidate, element)) return candidate;
    }
    throw new Error('Element selector is too deep');
  };
  const clip = text => {
    const encode = new TextEncoder();
    if (encode.encode(text).length <= 16384) return {text,truncated:false};
    let length = 0, result = '';
    for (const letter of text) {
      const bytes = encode.encode(letter).length;
      if (length + bytes > 16384) break;
      result += letter; length += bytes;
    }
    return {text:result,truncated:true};
  };
  const pick = event => {
    if (event.button !== 0 || !(event.target instanceof Element)) return;
    event.preventDefault(); event.stopImmediatePropagation();
    let element;
    try {
      const target = event.target, rect = target.getBoundingClientRect();
      element = {url:location.href,selector:selector(target),
        ...clip(String(target.innerText ?? '')),
        bounds:[rect.x,rect.y,rect.width,rect.height],viewport:[innerWidth,innerHeight]};
    } catch (_) {
      cancel(); send({kind:'cancelled',token}); return;
    }
    cancel();
    // The highlight must be gone before the native viewport snapshot begins.
    requestAnimationFrame(() => requestAnimationFrame(() => send({kind:'element',token,element})));
  };
  const key = event => {
    if (event.key === 'Escape') {
      event.preventDefault(); event.stopImmediatePropagation();
      cancel(); send({kind:'cancelled',token});
    }
  };
  document.addEventListener('pointermove', hover, true);
  for (const name of ['pointerdown','pointerup','mousedown','mouseup']) document.addEventListener(name, block, true);
  document.addEventListener('click', pick, true);
  document.addEventListener('keydown', key, true);
  window.__sailryElementSelection = {token,cancel};
}
