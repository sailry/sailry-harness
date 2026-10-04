import * as pdfjs from './pdfjs-dist/legacy/build/pdf.mjs';

export async function open(data, report) {
  const { EventBus, PDFViewer, PDFLinkService } = await import('./pdfjs-dist/legacy/web/pdf_viewer.mjs');
  const stylesheet = document.createElement('link');
  stylesheet.rel = 'stylesheet';
  stylesheet.href = 'pdfjs-dist/legacy/web/pdf_viewer.css';
  await new Promise((resolve, reject) => {
    stylesheet.onload = resolve;
    stylesheet.onerror = reject;
    document.head.prepend(stylesheet);
  });
  const container = document.querySelector('#container');
  const pages = document.querySelector('#pages');
  pages.className = 'pdfViewer';
  const eventBus = new EventBus();
  const linkService = new PDFLinkService({ eventBus });
  const viewer = new PDFViewer({
    container, viewer: pages, eventBus, linkService,
    maxCanvasPixels: 8 * 1024 * 1024, maxCanvasDim: 8192,
    annotationMode: pdfjs.AnnotationMode.ENABLE,
  });
  linkService.setViewer(viewer);
  const status = () => report({ pages: viewer.pagesCount, page: viewer.currentPageNumber, scale: viewer.currentScale, selection: (window.getSelection()?.toString() || '').slice(0, 16000) });
  document.addEventListener('selectionchange', status);
  eventBus.on('pagesinit', () => { viewer.currentScaleValue = 'page-width'; status(); });
  eventBus.on('pagechanging', status);
  eventBus.on('scalechanging', status);
  eventBus.on('pagerendered', event => { if (event.error) report({ error: 'failed', diagnostic: String(event.error) }); });
  // WebKit custom schemes cannot directly start workers. A same-view blob
  // containing the bundled module also works in WebView2, without a CDN.
  const workerSource = await (await fetch('pdfjs-dist/legacy/build/pdf.worker.mjs')).text();
  const workerUrl = URL.createObjectURL(new Blob([workerSource], { type: 'text/javascript' }));
  const worker = new Worker(workerUrl, { type: 'module' });
  pdfjs.GlobalWorkerOptions.workerPort = worker;
  const task = pdfjs.getDocument({
    data: new Uint8Array(data),
    cMapUrl: new URL('pdfjs-dist/cmaps/', location.href).href,
    cMapPacked: true,
    standardFontDataUrl: new URL('pdfjs-dist/standard_fonts/', location.href).href,
    wasmUrl: new URL('pdfjs-dist/wasm/', location.href).href,
    isEvalSupported: false,
    useWorkerFetch: false,
    enableXfa: false,
  });
  const pdfDocument = await task.promise;
  viewer.setDocument(pdfDocument);
  linkService.setDocument(pdfDocument);
  window.previewAction = action => {
    if (action === 'fit') viewer.currentScaleValue = 'page-width';
    if (action === 'in') viewer.currentScale = Math.min(3, viewer.currentScale * 1.2);
    if (action === 'out') viewer.currentScale = Math.max(0.25, viewer.currentScale / 1.2);
    if (action === 'next') viewer.currentPageNumber = Math.min(viewer.pagesCount, viewer.currentPageNumber + 1);
    if (action === 'previous') viewer.currentPageNumber = Math.max(1, viewer.currentPageNumber - 1);
    status();
  };
  new ResizeObserver(() => {
    if (viewer.currentScaleValue === 'page-width') viewer.currentScaleValue = 'page-width';
  }).observe(container);
  window.addEventListener('pagehide', () => {
    viewer.setDocument(null);
    task.destroy();
    worker.terminate();
    URL.revokeObjectURL(workerUrl);
  }, { once: true });
}
