// A page-local result promise; it exposes no native methods.
window.sailryExport = (() => {
  let complete;
  const result = new Promise(resolve => { complete = resolve; });
  addEventListener('error', event => { if (event.target instanceof HTMLScriptElement) complete({error:'Export script could not be loaded'}); }, true);
  addEventListener('unhandledrejection', event => complete({error:String(event.reason)}));
  return Object.freeze({result, complete});
})();
