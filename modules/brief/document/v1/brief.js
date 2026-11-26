(() => {
  window.MathJax?.loader?.preLoaded('[mathjax-tex]/svg');
  let current = null,generation = 0,serial = 0,edited = false,sections = [];
  const sources = new Map(),pending = new Set(),completed = new Set();
  const main = document.getElementById('brief'),toc = document.getElementById('document-toc');
  const sidebar = document.getElementById('document-sidebar'),menu = document.getElementById('document-menu');
  // Commands whose brace argument is a name or text rather than math identifiers.
  const named = new Set(['text','textrm','textit','textbf','texttt','textsf','textnormal','mbox','hbox',
    'mathrm','mathit','mathbf','mathsf','mathtt','mathcal','mathbb','mathfrak','mathscr','boldsymbol',
    'operatorname','begin','end','label','ref','eqref','tag','color','textcolor']);

  // TeX treats a word as a product of italic variables; set plain words upright.
  // Runs of three or more letters are words; shorter runs such as mc or ab stay math.
  function upright(source) {
    const command = /\\(?:[A-Za-z]+\*?|.?)/y,letters = /[A-Za-z]+/y;
    let output = '',index = 0;
    while (index < source.length) {
      if (source[index] === '\\') {
        command.lastIndex = index;
        const name = command.exec(source)[0];
        output += name;index += name.length;
        if (!named.has(name.slice(1).replace('*',''))) continue;
        while (/\s/.test(source[index] ?? '')) output += source[index++];
        if (source[index] !== '{') continue;
        let depth = 0;
        do {
          if (source[index] === '\\') output += source[index++];
          else if (source[index] === '{') depth++;
          else if (source[index] === '}') depth--;
          output += source[index++] ?? '';
        } while (depth > 0 && index < source.length);
        continue;
      }
      letters.lastIndex = index;
      const word = letters.exec(source)?.[0];
      if (!word) {output += source[index++];continue;}
      output += word.length >= 3 ? `\\mathrm{${word}}` : word;index += word.length;
    }
    return output;
  }
  const post = event => window.sailryDocumentPost?.(event);

  function theme(value) {
    for (const [key,color] of Object.entries(value)) {
      if (!/^[a-z_]+$/.test(key) || typeof color !== 'string' || color.length > 512) continue;
      document.documentElement.style.setProperty(`--document-${key.replaceAll('_','-')}`,color);
    }
  }

  function structure() {
    toc.replaceChildren();toc.setAttribute('aria-label',current.labels.contents);
    document.getElementById('document-title').textContent = current.identity.title;
    document.getElementById('document-contents').textContent = current.labels.contents;
    document.getElementById('document-menu-label').textContent = current.labels.contents;
    open(false);
    sections = [];
    const ids = new Set([...document.querySelectorAll('[id]')].map(element => element.id));
    for (const [index,section] of [...main.querySelectorAll('section[data-title]')].entries()) {
      const title = section.dataset.title;
      if (!section.id) {
        let suffix = index;
        while (ids.has(`document-section-${suffix}`)) suffix++;
        section.id = `document-section-${suffix}`;ids.add(section.id);
      }
      if (!section.querySelector('h1,h2,h3,h4')) {
        const heading = document.createElement('h2');heading.textContent = title;section.prepend(heading);
      }
      const link = document.createElement('a');link.href = `#${encodeURIComponent(section.id)}`;link.textContent = title;toc.append(link);
      sections.push({section,link});
      const own = section.querySelector('h1,h2,h3,h4');
      for (const heading of section.querySelectorAll('h3')) {
        if (heading === own || heading.closest('section[data-title]') !== section || heading.closest('.option,details,form,.callout')) continue;
        if (!heading.id) {
          let suffix = 0;
          while (ids.has(`document-heading-${suffix}`)) suffix++;
          heading.id = `document-heading-${suffix}`;ids.add(heading.id);
        }
        const item = document.createElement('a');item.className = 'sub';
        item.href = `#${encodeURIComponent(heading.id)}`;item.textContent = heading.textContent.trim();toc.append(item);
        sections.push({section:heading,link:item});
      }
    }
    sidebar.toggleAttribute('hidden',!sections.length);
    menu.toggleAttribute('hidden',!sections.length);
    document.getElementById('document-layout').setAttribute('data-contents',sections.length ? 'some' : 'none');
    for (const option of main.querySelectorAll('.compare > .option[data-title]')) {
      if (option.querySelector('h1,h2,h3,h4')) continue;
      const heading = document.createElement('h3');heading.textContent = option.dataset.title;option.prepend(heading);
    }
    for (const table of main.querySelectorAll('table')) {
      const wrapper = document.createElement('div');wrapper.className = 'document-table';table.replaceWith(wrapper);wrapper.append(table);
    }
    for (const label of main.querySelectorAll('label')) {
      label.classList.toggle('document-control',[...label.children].some(child => ['INPUT','SELECT'].includes(child.tagName)));
    }
    for (const form of main.querySelectorAll('form')) {
      for (const control of form.querySelectorAll('input,textarea,select')) {
        if (control.tagName === 'TEXTAREA' || control.tagName === 'INPUT' && control.type === 'text') control.maxLength = 4096;
      }
      for (const button of form.querySelectorAll('button')) {button.type = 'submit';button.textContent = current.labels.submit;}
    }
    writable(current.writable);
    activeSection();
  }

  function activeSection() {
    const selected = sections.findLast(item => item.section.getBoundingClientRect().top <= 96) ?? sections[0];
    for (const item of sections) {
      item.link.classList.toggle('active',item === selected);
      if (item === selected) item.link.setAttribute('aria-current','location');
      else item.link.removeAttribute('aria-current');
    }
  }
  document.addEventListener('scroll',activeSection,{passive:true});

  // The narrow-width contents menu reuses the sidebar as its dropdown panel.
  function open(value) {
    sidebar.toggleAttribute('data-open',value);
    menu.setAttribute('aria-expanded',String(value));
  }
  const opened = () => sidebar.hasAttribute('data-open');
  document.addEventListener('keydown',event => {
    if (event.key === 'Escape' && opened()) {open(false);menu.focus();}
  });

  function writable(value) {
    if (current) current.writable = value === true;
    for (const form of main.querySelectorAll('form')) {
      const disabled = value !== true || pending.has(form.dataset.name) || completed.has(form.dataset.name);
      for (const control of form.querySelectorAll('button,input,textarea,select')) control.disabled = disabled;
    }
  }

  function diagram(markup,palette) {
    if (!palette.diagram_foreground || !palette.diagram_background) return markup;
    const svg = new DOMParser().parseFromString(markup,'image/svg+xml').documentElement;
    const foreground = palette.diagram_foreground;
    const background = palette.diagram_background === 'transparent' ? 'none' : palette.diagram_background;
    const paint = (selector,fill,stroke) => {
      for (const element of svg.querySelectorAll(selector)) {
        element.style.setProperty('fill',fill,'important');
        element.style.setProperty('stroke',stroke,'important');
      }
    };
    svg.style.setProperty('background',palette.diagram_background,'important');
    svg.style.setProperty('color',foreground,'important');
    for (const element of [svg,...svg.querySelectorAll('*')]) {
      element.style.setProperty('filter','none','important');
      element.style.setProperty('text-shadow','none','important');
      element.style.setProperty('box-shadow','none','important');
    }
    paint('.node rect,.node circle,.node ellipse,.node polygon,.node path,.node line,.node polyline,.cluster rect',background,foreground);
    paint('.flowchart-link,.edgePath path','none',foreground);
    paint('marker path,marker circle,marker polygon',foreground,foreground);
    paint('text,tspan',foreground,'none');
    paint('.edgeLabel rect,.label rect,.labelBkg',background,'none');
    return svg.outerHTML;
  }

  async function render() {
    const token = ++generation;
    const palette = current.theme;
    window.mermaid.initialize({startOnLoad:false,securityLevel:'strict',htmlLabels:false,maxTextSize:65536,maxEdges:500,
      theme:'base',themeVariables:{background:palette.background,primaryColor:palette.muted,
        primaryTextColor:palette.foreground,primaryBorderColor:palette.border,lineColor:palette.foreground,
        secondaryColor:palette.secondary,tertiaryColor:palette.accent,edgeLabelBackground:palette.background,
        fontFamily:palette.font_family},flowchart:{htmlLabels:false}});
    for (const [element,source] of sources) {
      if (token !== generation) return;
      try {
        let svg;
        if (element.dataset.kind === 'mermaid') {
          if (source.includes('%%{') || /^\s*---/.test(source) || new TextEncoder().encode(source).length > 65536) throw Error('Unsupported diagram configuration');
          svg = diagram((await window.mermaid.render(`document-diagram-${serial++}`,source)).svg,palette);
        } else if (['math','math-inline'].includes(element.dataset.kind)) {
          if (new TextEncoder().encode(source).length > 65536) throw Error('Formula exceeds the supported size');
          await window.MathJax.startup.promise;
          const formula = await window.MathJax.tex2svgPromise(upright(source),{display:element.dataset.kind === 'math'});
          svg = formula.querySelector('svg').outerHTML;
        } else svg = window.sailryCharts.render(source,palette,Math.max(320,main.clientWidth - 32));
        if (token !== generation) return;
        element.innerHTML = svg;
      } catch (_) {
        if (token !== generation) return;
        const fallback = document.createElement('pre');fallback.textContent = source;element.replaceChildren(fallback);
      }
    }
  }

  async function mount(value) {
    generation++;current = value;edited = false;sources.clear();pending.clear();completed.clear();
    document.title = value.identity.title;theme(value.theme);main.innerHTML = value.html;structure();
    for (const draft of value.drafts ?? []) {
      const form = [...main.querySelectorAll('form')].find(form => form.dataset.name === draft.form);
      if (form && window.sailryForms.restore(form,draft)) edited = true;
    }
    for (const source of main.querySelectorAll('pre.mermaid,pre.chart,pre.math,span.math-inline')) {
      const kind = ['mermaid','chart','math','math-inline'].find(kind => source.classList.contains(kind));
      const element = document.createElement(kind === 'math-inline' ? 'span' : 'div');
      element.className = kind === 'mermaid' ? 'document-diagram' : kind === 'chart' ? 'document-chart' : `document-${kind}`;
      element.dataset.kind = kind;
      sources.set(element,source.textContent);source.replaceWith(element);
    }
    await render();
  }

  document.addEventListener('click',event => {
    if (event.target.closest('#document-menu')) {open(!opened());return;}
    if (opened() && !event.target.closest('#document-sidebar')) open(false);
    const anchor = event.target.closest('a[href^="#"]');
    if (anchor) {
      event.preventDefault();let id = anchor.getAttribute('href').slice(1);
      try {id = decodeURIComponent(id);} catch (_) {}
      if (opened()) open(false);
      [...main.querySelectorAll('[id]')].find(element => element.id === id)?.scrollIntoView();return;
    }
  });
  function markEdited() {
    if (edited || !current?.writable) return;
    edited = true;post({kind:'edited',...current.identity});
  }
  const edit = event => {
    const form = event.target.closest('form');
    if (!form || !current?.writable || pending.has(form.dataset.name) || completed.has(form.dataset.name)) return;
    markEdited();
    try {post({kind:'draft',...current.identity,...window.sailryForms.snapshot(form)});}
    catch (_) {post({kind:'invalid_form',...current.identity});}
  };
  document.addEventListener('input',edit);document.addEventListener('change',edit);
  document.addEventListener('submit',event => {
    event.preventDefault();
    const form = event.target;
    if (!current?.writable || pending.has(form.dataset.name) || completed.has(form.dataset.name)) return;
    try {
      const result = window.sailryForms.collect(form);
      if (result.missing) {
        result.missing.querySelector('input,textarea,select')?.focus();
        post({kind:'required',...current.identity});return;
      }
      markEdited();
      pending.add(result.form);writable(current.writable);
      post({kind:'submit',...current.identity,...result});
    } catch (_) {post({kind:'invalid_form',...current.identity});}
  });

  async function readonlyExport() {
    const clone = main.cloneNode(true);
    const liveForms = [...main.querySelectorAll('form')];
    for (const [formIndex,form] of [...clone.querySelectorAll('form')].entries()) {
      // cloneNode retains textarea defaults rather than the live value.
      const live = liveForms[formIndex];
      const values = live ? [...live.querySelectorAll('input,textarea,select')] : [];
      for (const [index,control] of [...form.querySelectorAll('input,textarea,select')].entries()) {
        const value = values[index];
        if (!value) continue;
        if (control.tagName === 'SELECT') for (const [index,option] of [...control.options].entries()) option.selected = value.options[index].selected;
        else {control.value = value.value;if (control.tagName === 'INPUT') control.checked = value.checked;}
      }
      form.replaceWith(window.sailryForms.readonly(form));
    }
    return clone.outerHTML;
  }
  window.sailryDocument = {mount,writable,setWritable:writable,readonlyExport,
    async retheme(value) {current.theme = value;theme(value);await render();},
    submitted(form,accepted) {pending.delete(form);if (accepted) completed.add(form);writable(current.writable);}};
})();
