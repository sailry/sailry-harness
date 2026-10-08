// Kit owns the controls; this adapter reports scrolling and performs their actions.
export function mount(pages, report) {
  const container = document.querySelector('#container');
  const items = pages.map(content => {
    const frame = document.createElement('div');
    frame.className = 'preview-frame';
    content.classList.add('preview-page');
    content.replaceWith(frame);
    frame.append(content);
    const width = content.offsetWidth;
    const height = content.offsetHeight;
    if (!width || !height) throw new Error('Document layout is empty');
    return { content, frame, width, height };
  });
  let scale = 1;
  let fitted = true;
  let page = 1;
  let pendingScroll = null;
  const status = () => report({
    pages: items.length, page, scale,
    selection: (window.getSelection()?.toString() || '').slice(0, 16000),
  });
  const layout = () => {
    if (fitted) scale = Math.min(1, Math.max(0.1, (container.clientWidth - 32) / Math.max(...items.map(item => item.width))));
    for (const { content, frame, width, height } of items) {
      Object.assign(content.style, { transform: `scale(${scale})`, transformOrigin: 'top left' });
      Object.assign(frame.style, { width: `${width * scale}px`, height: `${height * scale}px` });
    }
    status();
  };
  const navigate = number => {
    page = Math.max(1, Math.min(items.length, number));
    const before = container.scrollTop;
    container.scrollTop = items[page - 1].frame.offsetTop;
    if (container.scrollTop !== before) pendingScroll = container.scrollTop;
    status();
  };
  window.previewAction = action => {
    if (action === 'next') return navigate(page + 1);
    if (action === 'previous') return navigate(page - 1);
    if (action === 'fit') fitted = true;
    if (action === 'in' || action === 'out') {
      fitted = false;
      scale = Math.max(0.1, Math.min(3, scale * (action === 'in' ? 1.2 : 1 / 1.2)));
    }
    layout();
  };
  container.addEventListener('scroll', () => {
    // Browser clamping must not replace the page explicitly requested by Kit.
    if (pendingScroll !== null && container.scrollTop === pendingScroll) {
      pendingScroll = null;
      status();
      return;
    }
    pendingScroll = null;
    const bottom = container.scrollHeight - container.clientHeight;
    page = bottom > 0 && container.scrollTop >= bottom - 1
      ? items.length
      : items.reduce((current, item, index) => item.frame.offsetTop <= container.scrollTop + 16 ? index + 1 : current, 1);
    status();
  });
  document.addEventListener('selectionchange', status);
  const observer = new ResizeObserver(layout);
  observer.observe(container);
  window.addEventListener('pagehide', () => observer.disconnect(), { once: true });
  layout();
}
