import { mount } from './pages.mjs';

async function script(path) {
  await new Promise((resolve, reject) => {
    const node = document.createElement('script');
    node.src = path;
    node.onload = resolve;
    node.onerror = reject;
    document.head.append(node);
  });
}

export async function open(data, report, format) {
  const target = document.querySelector('#pages');
  let pages;
  if (format === 'docx') {
    await script('jszip/dist/jszip.min.js');
    await script('docx-preview/dist/docx-preview.min.js');
    await window.docx.renderAsync(data, target, document.head, {
      inWrapper: false,
      ignoreLastRenderedPageBreak: false,
      renderAltChunks: false,
      renderComments: false,
    });
    pages = [...target.querySelectorAll('section.docx')];
  } else if (format === 'xlsx') {
    const { read, utils } = await import('./xlsx/xlsx.mjs');
    const book = read(data, { cellFormula: true, cellHTML: false });
    pages = book.SheetNames.map(name => {
      const page = document.createElement('section');
      page.className = 'worksheet';
      const title = document.createElement('h2');
      title.textContent = name;
      page.append(title);
      const sheet = book.Sheets[name];
      // Cached formula values are displayed; preview never executes formulas.
      const parsed = new DOMParser().parseFromString(utils.sheet_to_html(sheet), 'text/html');
      const table = parsed.querySelector('table');
      if (table) {
        // File-authored links are content, not navigation or network capability.
        for (const link of table.querySelectorAll('a')) link.replaceWith(...link.childNodes);
        page.append(table);
      }
      target.append(page);
      return page;
    });
  } else if (format === 'pptx') {
    await script('jszip/dist/jszip.min.js');
    const { pptxToHtml } = await import('./@jvmr/pptx-to-html/dist/index.js');
    const slides = await pptxToHtml(data);
    pages = slides.map(html => {
      const fragment = new DOMParser().parseFromString(html, 'text/html');
      const page = fragment.body.firstElementChild;
      if (!page) throw new Error('Slide content is missing');
      target.append(page);
      return page;
    });
  } else {
    throw new Error('Unsupported document format');
  }
  if (!pages.length) throw new Error('Document has no viewable content');
  await document.fonts.ready;
  mount(pages, report);
}
