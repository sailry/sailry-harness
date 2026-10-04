// Invoked only by the typed browser adapter. Never evaluate model-provided source.
(request) => {
  try {
    if (request.action === "frame" && request.element == null) {
      window.__sailryBrowserFrame = null;
      window.__sailryBrowserSnapshot = null;
      return JSON.stringify({ ok: true });
    }
    if (request.action === "back") { window.__sailryBrowserFrame = null; history.back(); return JSON.stringify({ ok: true }); }
    if (request.action === "forward") { window.__sailryBrowserFrame = null; history.forward(); return JSON.stringify({ ok: true }); }
    if (request.action === "refresh") { window.__sailryBrowserFrame = null; location.reload(); return JSON.stringify({ ok: true }); }
    const frame = window.__sailryBrowserFrame;
    let ancestor = frame;
    while (ancestor) {
      if (!ancestor.isConnected) throw new Error("Frame changed; return to the top document");
      const owner = ancestor.ownerDocument.defaultView;
      if (!owner || owner.document !== ancestor.ownerDocument) throw new Error("Frame changed; return to the top document");
      if (owner === window) break;
      ancestor = owner.frameElement;
      if (!ancestor) throw new Error("Frame changed; return to the top document");
    }
    const doc = frame ? frame.contentDocument : document;
    if (!doc || (frame && !frame.isConnected)) throw new Error("Frame changed; return to the top document");
    const view = doc.defaultView;
    const visible = element => {
      const rect = element.getBoundingClientRect();
      const style = view.getComputedStyle(element);
      return rect.width > 0 && rect.height > 0 && rect.bottom > 0 && rect.right > 0
        && rect.top < view.innerHeight && rect.left < view.innerWidth
        && style.visibility !== "hidden" && style.display !== "none";
    };
    if (request.action === "wait") {
      const condition = request.condition;
      let ready;
      if (condition.kind === "text") ready = (doc.body?.innerText || "").includes(condition.text);
      else {
        const found = doc.querySelector(condition.selector);
        ready = condition.kind === "hidden" ? !found || !visible(found) : !!found && visible(found);
      }
      return JSON.stringify({ ready });
    }
    if (request.action === "read") {
      const elements = [];
      const nodes = new Map();
      const candidates = doc.querySelectorAll('a[href],button,input,textarea,select,iframe,[role="button"],[role="link"],[contenteditable="true"]');
      let count = 0;
      for (const element of candidates) {
        if (!visible(element)) continue;
        count++;
        if (elements.length >= 100) continue;
        const id = elements.length + 1;
        nodes.set(id, element);
        const item = { id, tag: element.tagName.toLowerCase(), role: element.getAttribute("role"),
          label: (element.getAttribute("aria-label") || element.innerText || element.getAttribute("placeholder") || element.getAttribute("name") || element.getAttribute("title") || "").slice(0, 200),
          type: element.getAttribute("type"), disabled: !!element.disabled,
          href: element.tagName === "A" ? element.href : undefined };
        if (element.tagName === "SELECT") {
          item.options = [...element.options].slice(0, 100).map(option => ({ value: option.value, label: option.label.slice(0, 200), selected: option.selected, disabled: option.disabled }));
        }
        if (element.tagName === "IFRAME") item.accessible = !!element.contentDocument;
        elements.push(item);
      }
      const parts = [];
      let length = 0, visited = 0;
      const walker = doc.createTreeWalker(doc.body || doc.documentElement, NodeFilter.SHOW_TEXT);
      const range = doc.createRange();
      while (walker.nextNode() && length < 20000 && visited++ < 20000) {
        const node = walker.currentNode;
        if (!node.textContent.trim()) continue;
        range.selectNodeContents(node);
        const inView = [...range.getClientRects()].some(rect => rect.width > 0 && rect.height > 0
          && rect.bottom > 0 && rect.top < view.innerHeight && rect.right > 0 && rect.left < view.innerWidth);
        if (inView && view.getComputedStyle(node.parentElement).visibility !== "hidden") {
          const text = node.textContent.trim(); parts.push(text); length += text.length + 1;
        }
      }
      const text = parts.join("\n");
      window.__sailryBrowserSnapshot = { id: request.snapshot, nodes, doc, url: view.location.href };
      return JSON.stringify({ url: view.location.href, title: doc.title, snapshot: request.snapshot, in_frame: !!frame,
        viewport: { width: view.innerWidth, height: view.innerHeight, x: view.scrollX, y: view.scrollY },
        text: text.slice(0, 20000), elements, truncated: text.length > 20000 || count > 100 || visited >= 20000,
        has_more_above: view.scrollY > 0, has_more_below: view.scrollY + view.innerHeight < doc.documentElement.scrollHeight,
        frames: doc.querySelectorAll("iframe").length });
    }
    if (request.action === "scroll") {
      view.scrollBy(0, view.innerHeight * (request.direction === "up" ? -0.8 : 0.8));
      return JSON.stringify({ ok: true });
    }
    const snapshot = window.__sailryBrowserSnapshot;
    if (!snapshot || snapshot.id !== request.snapshot || snapshot.doc !== doc || snapshot.url !== view.location.href) throw new Error("Page changed; read it again before acting");
    const element = snapshot.nodes.get(request.element);
    if (!element?.isConnected || !visible(element) || element.disabled) throw new Error("Element is no longer available; read the page again");
    const changed = () => {
      element.dispatchEvent(new view.Event("input", { bubbles: true }));
      element.dispatchEvent(new view.Event("change", { bubbles: true }));
    };
    const input = element instanceof view.HTMLInputElement || element instanceof view.HTMLTextAreaElement;
    const editable = input && !element.readOnly && !(element instanceof view.HTMLInputElement && ["file", "checkbox", "radio", "button", "submit", "reset", "hidden"].includes(element.type));
    const setValue = value => {
      const prototype = element instanceof view.HTMLTextAreaElement ? view.HTMLTextAreaElement.prototype : view.HTMLInputElement.prototype;
      Object.getOwnPropertyDescriptor(prototype, "value").set.call(element, value);
    };
    if (request.action === "click") element.click();
    else if (request.action === "input") {
      if (editable) setValue(request.text);
      else if (element.isContentEditable) element.textContent = request.text;
      else throw new Error("Element does not accept text");
      changed();
    } else if (request.action === "select") {
      if (!(element instanceof view.HTMLSelectElement)) throw new Error("Element is not a select control");
      const option = [...element.options].find(option => option.value === request.value);
      if (!option || option.disabled || option.parentElement.disabled) throw new Error("Option is unavailable");
      element.value = request.value;
      changed();
    } else if (request.action === "hover") {
      for (const name of ["pointerover", "pointerenter", "mouseover", "mouseenter", "mousemove"]) {
        const EventType = name.startsWith("pointer") ? view.PointerEvent : view.MouseEvent;
        element.dispatchEvent(new EventType(name, { bubbles: !name.endsWith("enter"), view }));
      }
    } else if (request.action === "key") {
      element.focus();
      const options = { key: request.key, bubbles: true, cancelable: true };
      if (element.dispatchEvent(new view.KeyboardEvent("keydown", options))) {
        if (request.key === "Enter" && element.form && element.tagName !== "TEXTAREA") element.form.requestSubmit();
        else if (request.key === "Enter" && ["BUTTON", "A"].includes(element.tagName)) element.click();
        else if (request.key === "Tab") {
          const focusable = [...doc.querySelectorAll('a[href],button,input,textarea,select,[tabindex],[contenteditable="true"]')].filter(node => visible(node) && !node.disabled && node.tabIndex >= 0);
          if (focusable.length) focusable[(focusable.indexOf(element) + 1) % focusable.length].focus();
        } else if (request.key === "Backspace" && editable) {
          const end = element.selectionEnd ?? element.value.length;
          const start = element.selectionStart ?? end;
          const previous = [...new Intl.Segmenter(undefined, { granularity: "grapheme" }).segment(element.value.slice(0, start))].at(-1)?.index ?? 0;
          const from = start === end ? previous : start;
          setValue(element.value.slice(0, from) + element.value.slice(end));
          if (element.selectionStart !== null) element.setSelectionRange(from, from);
          changed();
        }
      }
      element.dispatchEvent(new view.KeyboardEvent("keyup", options));
    } else if (request.action === "frame") {
      if (!(element instanceof view.HTMLIFrameElement) || !element.contentDocument) throw new Error("Frame is cross-origin or unavailable; use external browser automation");
      window.__sailryBrowserFrame = element;
      window.__sailryBrowserSnapshot = null;
    } else throw new Error("Unsupported browser action");
    return JSON.stringify({ ok: true });
  } catch (error) { return JSON.stringify({ error: String(error.message || error) }); }
}
