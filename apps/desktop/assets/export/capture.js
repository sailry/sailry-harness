window.addEventListener('load', async () => {
  try {
    await window.sailryExportReady;
    await document.fonts.ready;
    // Canvas needs explicit pixel dimensions and self-contained SVG colors.
    for (const svg of document.querySelectorAll('svg')) {
      const bounds = svg.getBoundingClientRect();
      svg.setAttribute('width', String(bounds.width));
      svg.setAttribute('height', String(bounds.height));
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
    await Promise.all([...document.images].map(image => image.decode()));
    const height = Math.ceil(document.documentElement.scrollHeight);
    if (height <= 0 || height > 32768) throw Error('Export exceeds the supported image height');
    const tiles = [];
    for (let y = 0; y < height; y += 2048) {
      const canvas = await html2canvas(document.body, {x:0, y, width:960, height:Math.min(2048,height-y), windowWidth:960, windowHeight:768, scale:1, scrollX:0, scrollY:0, backgroundColor:'#ffffff', logging:false, imageTimeout:15000});
      tiles.push(canvas.toDataURL('image/png'));
      canvas.width = canvas.height = 0;
    }
    window.ipc.postMessage(JSON.stringify({height, tiles}));
  } catch (error) {
    window.ipc.postMessage(JSON.stringify({error: String(error?.message || error)}));
  }
}, {once:true});
