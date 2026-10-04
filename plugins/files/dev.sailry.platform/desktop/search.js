// Search draft/reset policy from Sailry 116aab0f's file_search.rs.
import {createText, readText, releaseText, focusText} from 'sailry/forms';
import {searchFiles, cancelFileSearch, faultCode} from 'sailry/sdk';

export class Search {
  constructor(text,report) {
    this.report = report;
    this.query = createText('',{placeholder:text.file_search_query,label:text.file_search_query});
    this.filter = createText('',{placeholder:text.file_search_filter,label:text.file_search_filter});
    this.regex = false; this.case_sensitive = false;
    this.result = null; this.running = false; this.status = null; this.generation = 0;
    focusText(this.query);
  }

  reset(cx) {
    ++this.generation; cancelFileSearch();
    this.result = null; this.running = false; this.status = null;
    cx.notify();
  }

  close(cx) { this.reset(cx); releaseText(this.query); releaseText(this.filter); }

  toggle(kind,cx) { this[kind] = !this[kind]; this.reset(cx); }

  async run(cx) {
    this.reset(cx);
    const options = {query:readText(this.query),regex:this.regex,case_sensitive:this.case_sensitive,
      globs:readText(this.filter).split('\n').map(value => value.trim()).filter(Boolean)};
    if (!options.query) return;
    const generation = this.generation;
    this.running = true; this.status = 'file_search_running'; cx.notify();
    try {
      const result = await searchFiles(options);
      if (generation !== this.generation) return;
      this.result = result;
      this.status = result.truncated || result.skipped > 0 ? 'file_search_partial'
        : !result.matches.length ? 'file_search_empty' : null;
    } catch (error) {
      if (generation !== this.generation) return;
      this.status = null;
      this.report(faultCode(error.message ?? String(error)) === 'invalid_request' ? 'file_search_invalid' : 'file_search_failed');
    } finally {
      if (generation === this.generation) { this.running = false; cx.notify(); }
    }
  }
}
