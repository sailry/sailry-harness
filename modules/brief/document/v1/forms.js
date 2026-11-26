(() => {
  const bytes = value => new TextEncoder().encode(value).length;
  const name = value => typeof value === 'string' && /^[A-Za-z][A-Za-z0-9_.-]{0,63}$/.test(value);
  const text = (value,limit) => typeof value === 'string' && bytes(value) <= limit && !/[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]/.test(value);

  function collect(form) {
    const id = form.dataset.name;
    if (!name(id)) throw Error('Invalid form name');
    if ([...form.ownerDocument?.querySelectorAll('form') ?? []]
      .some(item => item !== form && item.dataset.name === id)) throw Error('Duplicate form name');
    const fieldsets = [...form.querySelectorAll('fieldset')],fields = [],seen = new Set();
    if (!fieldsets.length || fieldsets.length > 32) throw Error('Invalid form fields');
    for (const fieldset of fieldsets) {
      const id = fieldset.dataset.name,label = fieldset.querySelector('legend')?.textContent?.trim() ?? '';
      if (!name(id) || seen.has(id) || !text(label,512)) throw Error('Invalid form field');
      seen.add(id);
      const controls = [...fieldset.querySelectorAll('input,textarea,select')];
      if (!controls.length || controls.length > 64 || controls.some(control => control.name !== id || control.disabled)) throw Error('Invalid form controls');
      const values = [];
      for (const control of controls) {
        if (control.tagName === 'INPUT' && ['radio','checkbox'].includes(control.type)) {
          if (!control.checked) continue;
          const label = control.closest('label')?.textContent?.trim() ?? control.value;
          if (!text(control.value,512) || !text(label,512)) throw Error('Invalid form choice');
          values.push({value:control.value,label});
        } else if (control.tagName === 'TEXTAREA' || control.tagName === 'INPUT' && control.type === 'text') {
          if (!text(control.value,4096)) throw Error('Form answer exceeds the supported size');
          if (control.value.trim()) values.push({value:control.value,label:control.value});
        } else if (control.tagName === 'SELECT') {
          for (const option of [...control.selectedOptions]) {
            const label = option.textContent.trim();
            if (!text(option.value,512) || !text(label,512)) throw Error('Invalid form choice');
            if (option.value) values.push({value:option.value,label});
          }
        } else throw Error('Unsupported form control');
      }
      if (fieldset.hasAttribute('data-required') && !values.length) return {missing:fieldset,form:form.dataset.name};
      fields.push({name:id,label,values});
    }
    const result = {form:id,fields};
    if (bytes(JSON.stringify(result)) > 16384) throw Error('Form response exceeds the supported size');
    return result;
  }

  function readonly(form) {
    const output = document.createElement('section');output.className = 'document-readonly';
    for (const fieldset of form.querySelectorAll('fieldset')) {
      const title = document.createElement('h3');title.textContent = fieldset.querySelector('legend')?.textContent ?? '';
      output.append(title);
      const list = document.createElement('ul');
      for (const control of fieldset.querySelectorAll('input,textarea,select')) {
        if (control.tagName === 'INPUT' && ['radio','checkbox'].includes(control.type)) {
          const line = document.createElement('li');line.textContent = `${control.checked ? '☑' : '☐'} ${control.closest('label')?.textContent?.trim() ?? control.value}`;list.append(line);
        } else if (control.tagName === 'SELECT') {
          for (const option of control.options) {
            const line = document.createElement('li');line.textContent = `${option.selected ? '☑' : '☐'} ${option.textContent}`;list.append(line);
          }
        } else {
          const line = document.createElement('li');line.textContent = control.value;list.append(line);
        }
      }
      output.append(list);
    }
    return output;
  }
  function snapshot(form) {
    if (!name(form.dataset.name)) throw Error('Invalid form name');
    const fields = [...form.querySelectorAll('fieldset')].map(fieldset => {
      const id = fieldset.dataset.name,controls = [...fieldset.querySelectorAll('input,textarea,select')];
      if (!name(id) || !controls.length || controls.length > 64 || controls.some(control => control.name !== id)) throw Error('Invalid form controls');
      const values = controls.flatMap(control => {
        if (control.tagName === 'INPUT' && ['radio','checkbox'].includes(control.type)) return control.checked ? [control.value] : [];
        if (control.tagName === 'SELECT') return [...control.selectedOptions].map(option => option.value);
        if (control.tagName === 'TEXTAREA' || control.tagName === 'INPUT' && control.type === 'text') return [control.value];
        throw Error('Unsupported form control');
      });
      if (values.length > 64 || values.some(value => !text(value,4096))) throw Error('Form draft exceeds the supported size');
      return {name:id,values};
    });
    const result = {form:form.dataset.name,fields};
    if (!fields.length || fields.length > 32 || new Set(fields.map(field => field.name)).size !== fields.length || bytes(JSON.stringify(result)) > 16384) throw Error('Invalid form draft');
    return result;
  }
  function restore(form,draft) {
    if (draft?.form !== form.dataset.name || !Array.isArray(draft.fields) || draft.fields.length > 32 || bytes(JSON.stringify(draft)) > 16384) return false;
    if (draft.fields.some(field => !name(field.name) || !Array.isArray(field.values) || field.values.length > 64 || field.values.some(value => !text(value,4096)))) return false;
    const fields = new Map(draft.fields.map(field => [field.name,field.values]));
    if (fields.size !== draft.fields.length) return false;
    let restored = false;
    for (const fieldset of form.querySelectorAll('fieldset')) {
      const values = fields.get(fieldset.dataset.name);
      if (!values) continue;
      let index = 0;
      for (const control of fieldset.querySelectorAll('input,textarea,select')) {
        if (control.name !== fieldset.dataset.name) continue;
        if (control.tagName === 'INPUT' && ['radio','checkbox'].includes(control.type)) control.checked = values.includes(control.value);
        else if (control.tagName === 'SELECT') for (const option of control.options) option.selected = values.includes(option.value);
        else if (control.tagName === 'TEXTAREA' || control.tagName === 'INPUT' && control.type === 'text') control.value = values[index++] ?? '';
        else continue;
        restored = true;
      }
    }
    return restored;
  }
  window.sailryForms = {collect,readonly,snapshot,restore};
})();
