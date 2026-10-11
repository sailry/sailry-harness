// Wry injects this into every frame, including sandboxed file-preview frames.
(() => {
  if (window.sailrySnapshotWatching) return;
  window.sailrySnapshotWatching = true;
  let timer, dirty = false;
  const send = state => {
    const message = `sailry-snapshot:${state}`;
    if (window.ipc) window.ipc.postMessage(message);
    // Wry exposes its ipc object only in the main frame on macOS, while
    // WKUserContentController's message handler also accepts subframe events.
    else if (window.webkit?.messageHandlers?.ipc) window.webkit.messageHandlers.ipc.postMessage(message);
    else window.chrome?.webview?.postMessage(message);
  };
  const changed = () => {
    if (!dirty) { send('dirty'); dirty = true; }
    clearTimeout(timer);
    timer = setTimeout(() => { dirty = false; send('ready'); }, 150);
  };
  addEventListener('scroll', changed, true);
  addEventListener('resize', changed);
  addEventListener('input', changed, true);
  addEventListener('change', changed, true);
  addEventListener('load', changed, true);
  const observe = () => {
    new MutationObserver(changed).observe(document.documentElement, {subtree:true, childList:true, attributes:true, characterData:true});
    new ResizeObserver(changed).observe(document.documentElement);
    document.fonts?.ready.then(changed);
    changed();
  };
  if (document.readyState === 'loading') addEventListener('DOMContentLoaded', observe, {once:true});
  else observe();
})();
