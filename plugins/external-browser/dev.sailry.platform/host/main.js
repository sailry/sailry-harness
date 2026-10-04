// Tool metadata in plugin.json is adapted from adk-browser b3e360bbf49ca39d49cccba182c0f7cb4d251970.
// Approval groups, captions and result shapes preserve Sailry e6f7041a's external-browser adapter.
// See NOTICE and LICENSE for source attribution.

function prepare(action, args) {
  if (!args || typeof args !== 'object' || Array.isArray(args)) {
    return {isError:true,error:{code:'invalid_request',message:'Browser arguments must be an object'}};
  }
  if (action === 'wait' && (!Number.isFinite(args.seconds) || args.seconds < 0 || args.seconds > 30)) {
    return {isError:true,error:{code:'invalid_request',message:'browser wait must be between 0 and 30 seconds'}};
  }
  // The Node owns URL validation, confined files, driver access and session lifetime.
  return {action, arguments:args};
}

export function navigate(args) { return prepare('navigate', args); }
export function back(args) { return prepare('back', args); }
export function forward(args) { return prepare('forward', args); }
export function refresh(args) { return prepare('refresh', args); }
export function click(args) { return prepare('click', args); }
export function double_click(args) { return prepare('double_click', args); }
export function type(args) { return prepare('type', args); }
export function clear(args) { return prepare('clear', args); }
export function select(args) { return prepare('select', args); }
export function extract_text(args) { return prepare('extract_text', args); }
export function extract_attribute(args) { return prepare('extract_attribute', args); }
export function extract_links(args) { return prepare('extract_links', args); }
export function page_info(args) { return prepare('page_info', args); }
export function page_source(args) { return prepare('page_source', args); }
export function wait_for_element(args) { return prepare('wait_for_element', args); }
export function wait(args) { return prepare('wait', args); }
export function wait_for_page_load(args) { return prepare('wait_for_page_load', args); }
export function wait_for_text(args) { return prepare('wait_for_text', args); }
export function screenshot(args) { return prepare('screenshot', args); }
export function evaluate_js(args) { return prepare('evaluate_js', args); }
export function scroll(args) { return prepare('scroll', args); }
export function hover(args) { return prepare('hover', args); }
export function handle_alert(args) { return prepare('handle_alert', args); }
export function get_cookies(args) { return prepare('get_cookies', args); }
export function get_cookie(args) { return prepare('get_cookie', args); }
export function add_cookie(args) { return prepare('add_cookie', args); }
export function delete_cookie(args) { return prepare('delete_cookie', args); }
export function delete_all_cookies(args) { return prepare('delete_all_cookies', args); }
export function list_windows(args) { return prepare('list_windows', args); }
export function new_tab(args) { return prepare('new_tab', args); }
export function new_window(args) { return prepare('new_window', args); }
export function switch_window(args) { return prepare('switch_window', args); }
export function close_window(args) { return prepare('close_window', args); }
export function maximize_window(args) { return prepare('maximize_window', args); }
export function minimize_window(args) { return prepare('minimize_window', args); }
export function set_window_size(args) { return prepare('set_window_size', args); }
export function switch_to_frame(args) { return prepare('switch_to_frame', args); }
export function switch_to_parent_frame(args) { return prepare('switch_to_parent_frame', args); }
export function switch_to_default_content(args) { return prepare('switch_to_default_content', args); }
export function drag_and_drop(args) { return prepare('drag_and_drop', args); }
export function right_click(args) { return prepare('right_click', args); }
export function focus(args) { return prepare('focus', args); }
export function element_state(args) { return prepare('element_state', args); }
export function press_key(args) { return prepare('press_key', args); }
export function file_upload(args) { return prepare('file_upload', args); }
export function print_to_pdf(args) { return prepare('print_to_pdf', args); }
export function close_session(args) { return prepare('close_session', args); }
export function downloads(args) { return prepare('downloads', args); }
export function save_download(args) { return prepare('save_download', args); }

export function result({output}) {
  return output.data;
}
