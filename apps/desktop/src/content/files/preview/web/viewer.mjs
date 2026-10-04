// The host owns every control. The renderer reports only its display state;
// it receives no filesystem paths, credentials or host execution capability.
export function report(status) {
  window.ipc.postMessage(JSON.stringify(status));
}
window.layoutPreview = () => {
  if (window.previewFrame) Object.assign(document.querySelector('#preview').style, window.previewFrame);
};
window.layoutPreview();
window.previewTheme = color => document.documentElement.style.setProperty('--preview-background', color);
window.previewTheme(window.previewBackground);
window.addEventListener('error', event => report({ error: 'failed', diagnostic: event.message }));
window.addEventListener('unhandledrejection', event => report({ error: 'failed', diagnostic: String(event.reason) }));
// Links in documents must not replace the preview or open another native view.
document.addEventListener('click', event => {
  if (event.target.closest('a')) event.preventDefault();
});
try {
  const response = await fetch('document');
  if (!response.ok) throw new Error('Document download failed');
  const data = await response.arrayBuffer();
  const renderer = await import('./pdf.mjs');
  await renderer.open(data, report);
} catch (error) {
  console.error('Document preview failed', error);
  report({ error: error?.name === 'PasswordException' ? 'password' : 'failed', diagnostic: String(error) });
}
