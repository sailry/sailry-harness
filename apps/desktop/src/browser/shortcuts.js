if (window.__sailryShortcutHandler) {
    removeEventListener('keydown', window.__sailryShortcutHandler, true);
}
let pending = null;
let deadline = 0;
const aliases = { ' ': 'space', arrowleft: 'left', arrowright: 'right', arrowup: 'up', arrowdown: 'down' };
const matches = (stroke, event) => {
    const key = event.key.toLowerCase();
    return stroke.key === (aliases[key] || key) && stroke.meta === event.metaKey &&
        stroke.ctrl === event.ctrlKey && stroke.alt === event.altKey && stroke.shift === event.shiftKey;
};
window.__sailryShortcutHandler = event => {
    let action = null;
    if (pending && performance.now() <= deadline) {
        action = pending.find(binding => matches(binding.strokes[1], event))?.action;
    }
    pending = null;
    if (!action) {
        const candidates = bindings.filter(binding => matches(binding.strokes[0], event));
        action = candidates.find(binding => binding.strokes.length === 1)?.action;
        if (!action && candidates.length) {
            pending = candidates;
            deadline = performance.now() + 1000;
        }
    }
    if (action || pending) {
        event.preventDefault();
        event.stopPropagation();
        if (action) window.ipc.postMessage(action);
    }
};
addEventListener('keydown', window.__sailryShortcutHandler, true);
