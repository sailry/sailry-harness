// flutter_svg 2.3.0 does not render embedded CSS/class selectors or SVG markers.
// Rasterize at native pixel density while retaining bounded bitmap memory.
const diagramRasterizer = r'''
window.sailryVisual = async request => {
  const result = await window.sailryRender(request);
  if (request.kind !== 'mermaid') return result;
  const image = new Image();
  const canvas = document.createElement('canvas');
  try {
    await new Promise((resolve, reject) => {
      image.onload = resolve;
      image.onerror = () => reject(new Error('Diagram image could not load'));
      image.src = 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(result.source);
    });
    const scale = Math.min(Math.max(1, window.devicePixelRatio || 1),
      4096 / Math.max(result.width, result.height),
      Math.sqrt(2097152 / (result.width * result.height)));
    canvas.width = Math.max(1, Math.floor(result.width * scale));
    canvas.height = Math.max(1, Math.floor(result.height * scale));
    const context = canvas.getContext('2d');
    if (!context) throw new Error('Diagram canvas is unavailable');
    context.drawImage(image, 0, 0, canvas.width, canvas.height);
    const data = canvas.toDataURL('image/png');
    const prefix = 'data:image/png;base64,';
    if (!data.startsWith(prefix) || data.length - prefix.length > 5592408) {
      throw new Error('Rendered diagram exceeds its PNG size limit');
    }
    return {...result, png: data.slice(prefix.length)};
  } finally {
    image.src = '';
    canvas.width = canvas.height = 0;
  }
};
''';
