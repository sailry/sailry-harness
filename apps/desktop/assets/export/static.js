window.addEventListener('load', async () => {
  try {
    await window.sailryExportReady;
    await document.fonts.ready;
    const inline = async url => {
      const response = await fetch(url);
      if (!response.ok) throw Error('Export attachment could not be read');
      const blob = await response.blob();
      if (blob.size > 32 * 1024 * 1024) throw Error('Export attachment exceeds the supported size');
      return await new Promise((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => resolve(reader.result);
        reader.onerror = () => reject(Error('Export attachment could not be embedded'));
        reader.readAsDataURL(blob);
      });
    };
    // Copy shared glyph definitions into each SVG so individual visuals remain complete.
    for (const svg of document.querySelectorAll('svg')) {
      svg.style.color = getComputedStyle(svg).color;
      for (const use of svg.querySelectorAll('use')) {
        const reference = use.getAttribute('href') || use.getAttribute('xlink:href');
        if (!reference?.startsWith('#')) continue;
        const target = document.getElementById(reference.slice(1));
        if (!target || svg.contains(target)) continue;
        let definitions = svg.querySelector('defs');
        if (!definitions) {
          definitions = document.createElementNS('http://www.w3.org/2000/svg', 'defs');
          svg.prepend(definitions);
        }
        if (![...definitions.children].some(child => child.id === target.id)) definitions.append(target.cloneNode(true));
      }
    }
    for (const image of document.querySelectorAll('img[src]')) {
      if (!image.getAttribute('src').startsWith('data:')) image.setAttribute('src', await inline(image.src));
    }
    for (const link of document.querySelectorAll('a[href]')) {
      const href = link.getAttribute('href');
      if (href.startsWith('sailry-assets-')) {
        link.setAttribute('download', href.split('/').pop());
        link.setAttribute('href', await inline(link.href));
      }
    }
    for (const element of document.querySelectorAll('script,meta[http-equiv="Content-Security-Policy"]')) element.remove();
    const policy = document.createElement('meta');
    policy.httpEquiv = 'Content-Security-Policy';
    policy.content = "default-src 'none'; script-src 'none'; style-src 'unsafe-inline'; img-src data:; font-src data:; connect-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'";
    document.head.prepend(policy);
    const html = '<!doctype html>\n' + document.documentElement.outerHTML;
    if (html.length > 48 * 1024 * 1024) throw Error('Export document exceeds the supported size');
    window.ipc.postMessage(JSON.stringify({html}));
  } catch (error) {
    window.ipc.postMessage(JSON.stringify({error: String(error?.message || error)}));
  }
}, {once:true});
