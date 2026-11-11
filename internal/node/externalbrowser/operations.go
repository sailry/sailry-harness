package externalbrowser

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/chromedp/cdproto/cdp"
	"github.com/chromedp/cdproto/dom"
	"github.com/chromedp/cdproto/page"
	"github.com/chromedp/cdproto/runtime"
	"github.com/chromedp/cdproto/target"
	"github.com/chromedp/chromedp"
	"github.com/chromedp/chromedp/kb"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type operation func(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error)

var operations map[protocol.ExternalBrowserAction]operation

func init() {
	operations = map[protocol.ExternalBrowserAction]operation{
		protocol.ExternalBrowserActionNavigate:               navigate,
		protocol.ExternalBrowserActionBack:                   history(chromedp.NavigateBack(), "Back navigation failed"),
		protocol.ExternalBrowserActionForward:                history(chromedp.NavigateForward(), "Forward navigation failed"),
		protocol.ExternalBrowserActionRefresh:                history(chromedp.Reload(), "Refresh failed"),
		protocol.ExternalBrowserActionClick:                  click,
		protocol.ExternalBrowserActionDoubleClick:            doubleClick,
		protocol.ExternalBrowserActionRightClick:             rightClick,
		protocol.ExternalBrowserActionHover:                  hover,
		protocol.ExternalBrowserActionFocus:                  focus,
		protocol.ExternalBrowserActionType:                   typeText,
		protocol.ExternalBrowserActionClear:                  clearField,
		protocol.ExternalBrowserActionSelect:                 selectOption,
		protocol.ExternalBrowserActionPressKey:               pressKey,
		protocol.ExternalBrowserActionDragAndDrop:            dragAndDrop,
		protocol.ExternalBrowserActionFileUpload:             fileUpload,
		protocol.ExternalBrowserActionScroll:                 scroll,
		protocol.ExternalBrowserActionExtractText:            extractText,
		protocol.ExternalBrowserActionExtractAttribute:       extractAttribute,
		protocol.ExternalBrowserActionExtractLinks:           extractLinks,
		protocol.ExternalBrowserActionElementState:           elementState,
		protocol.ExternalBrowserActionPageInfo:               pageInfo,
		protocol.ExternalBrowserActionPageSource:             pageSource,
		protocol.ExternalBrowserActionEvaluateJs:             evaluate,
		protocol.ExternalBrowserActionScreenshot:             screenshot,
		protocol.ExternalBrowserActionPrintToPdf:             printToPDF,
		protocol.ExternalBrowserActionHandleAlert:            handleAlert,
		protocol.ExternalBrowserActionWait:                   wait,
		protocol.ExternalBrowserActionWaitForElement:         waitForElement,
		protocol.ExternalBrowserActionWaitForPageLoad:        waitForPageLoad,
		protocol.ExternalBrowserActionWaitForText:            waitForText,
		protocol.ExternalBrowserActionGetCookies:             getCookies,
		protocol.ExternalBrowserActionGetCookie:              getCookie,
		protocol.ExternalBrowserActionAddCookie:              addCookie,
		protocol.ExternalBrowserActionDeleteCookie:           deleteCookie,
		protocol.ExternalBrowserActionDeleteAllCookies:       deleteAllCookies,
		protocol.ExternalBrowserActionListWindows:            listWindows,
		protocol.ExternalBrowserActionNewTab:                 newWindow(false),
		protocol.ExternalBrowserActionNewWindow:              newWindow(true),
		protocol.ExternalBrowserActionSwitchWindow:           switchWindow,
		protocol.ExternalBrowserActionCloseWindow:            closeWindow,
		protocol.ExternalBrowserActionMaximizeWindow:         windowState("maximized", "Window maximized"),
		protocol.ExternalBrowserActionMinimizeWindow:         windowState("minimized", "Window minimized"),
		protocol.ExternalBrowserActionSetWindowSize:          setWindowSize,
		protocol.ExternalBrowserActionSwitchToFrame:          switchToFrame,
		protocol.ExternalBrowserActionSwitchToParentFrame:    switchToParentFrame,
		protocol.ExternalBrowserActionSwitchToDefaultContent: switchToDefaultContent,
	}
}

func (session *session) execute(ctx context.Context, action protocol.ExternalBrowserAction, arguments map[string]any) (map[string]any, error) {
	run := operations[action]
	if run == nil {
		return nil, fault(protocol.ErrorCodeInvalidRequest, "unknown browser action")
	}
	result, err := run(ctx, session, arguments)
	if err != nil {
		var known *protocol.Fault
		if errors.As(err, &known) {
			return nil, known
		}
		return nil, unavailable(err.Error())
	}
	plain, _ := normalize(result).(map[string]any)
	return plain, nil
}

func failed(prefix string, err error) error {
	var known *protocol.Fault
	if errors.As(err, &known) {
		return err
	}
	return unavailable(prefix + ": " + err.Error())
}

// bind derives a context of one browser target that also ends with the
// operation and after limit.
func bind(ctx, target context.Context, limit time.Duration) (context.Context, context.CancelFunc) {
	bound, cancel := context.WithTimeout(target, limit)
	stop := context.AfterFunc(ctx, cancel)
	return bound, func() { stop(); cancel() }
}

// top binds the current page target, whatever frame is selected.
func (session *session) top(ctx context.Context, limit time.Duration) (context.Context, context.CancelFunc, error) {
	if session.current == nil || session.current.ctx.Err() != nil {
		return nil, nil, unavailable("no browser window is open; open a new tab")
	}
	bound, cancel := bind(ctx, session.current.ctx, limit)
	return bound, cancel, nil
}

// within returns the query options that confine a selector to the frame.
func within(owner *chromedp.Node, options ...chromedp.QueryOption) []chromedp.QueryOption {
	if owner != nil {
		options = append(options, chromedp.FromNode(owner))
	}
	return options
}

// call runs a JavaScript function in the selected frame's document with one
// JSON argument and returns its JSON result. Promises are awaited.
func (session *session) call(ctx context.Context, limit time.Duration, function string, argument any) (any, error) {
	target, owner, err := session.scope()
	if err != nil {
		return nil, err
	}
	bound, cancel := bind(ctx, target, limit)
	defer cancel()
	if owner == nil {
		encoded, err := json.Marshal(argument)
		if err != nil {
			return nil, err
		}
		return chromedp.Run(bound, chromedp.Evaluate[any]("("+function+").call(document.documentElement,"+string(encoded)+")", chromedp.EvalAwaitPromise))
	}
	return chromedp.Run(bound, chromedp.QueryAfter(chromedp.CSS("html"), func(ctx context.Context, t *chromedp.Target, nodes []*chromedp.Node) (any, error) {
		return callOn(ctx, t, nodes[0], function, argument)
	}, chromedp.FromNode(owner)))
}

// callOn runs a function with a node as this.
func callOn(ctx context.Context, t *chromedp.Target, node *chromedp.Node, function string, arguments ...any) (any, error) {
	resolved, err := cdp.Call(ctx, t, dom.ResolveNode, dom.ResolveNodeParams{NodeID: node.NodeID})
	if err != nil {
		return nil, err
	}
	defer func() {
		_, _ = cdp.Call(ctx, t, runtime.ReleaseObject, runtime.ReleaseObjectParams{ObjectID: resolved.Object.ObjectID})
	}()
	await := true
	return chromedp.CallFunctionOn[any](function, func(params *runtime.CallFunctionOnParams) {
		params.ObjectID = resolved.Object.ObjectID
		params.AwaitPromise = &await
	}, arguments...)(ctx, t)
}

// element waits up to limit for the first node a selector matches in the
// selected frame, then runs use with it on the node's target.
func (session *session) element(ctx context.Context, selector string, limit time.Duration, visible bool, use func(ctx context.Context, t *chromedp.Target, node *chromedp.Node) error) error {
	target, owner, err := session.scope()
	if err != nil {
		return err
	}
	options := within(owner)
	if visible {
		options = append(options, chromedp.NodeVisible)
	}
	waiting, cancel := bind(ctx, target, max(limit, time.Second))
	nodes, err := chromedp.Run(waiting, chromedp.Nodes(chromedp.CSS(selector), options...))
	cancel()
	if err != nil {
		if ctx.Err() == nil && errors.Is(err, context.DeadlineExceeded) {
			return &missing{unavailable("Element not found: " + selector)}
		}
		return failed("Element lookup failed", err)
	}
	acting, cancel := bind(ctx, target, operationTimeout)
	defer cancel()
	return chromedp.Do(acting, chromedp.Func(func(ctx context.Context, t *chromedp.Target) error {
		return use(ctx, t, nodes[0])
	}))
}

// errNotFound marks the fault of a selector that matched nothing in time.
var errNotFound = errors.New("element not found")

// missing is that fault. It still reads as the fault it wraps, so every
// caller reports it unchanged, and a wait can tell it from a failure.
type missing struct{ fault error }

func (missing *missing) Error() string        { return missing.fault.Error() }
func (missing *missing) Unwrap() error        { return missing.fault }
func (missing *missing) Is(target error) bool { return target == errNotFound }

// waited reports that a page condition did not hold within its own limit
// while the call itself was not interrupted.
func waited(ctx context.Context, err error) bool {
	return ctx.Err() == nil && (errors.Is(err, chromedp.ErrPollingTimeout) || errors.Is(err, context.DeadlineExceeded))
}

// center scrolls a node into view and returns its middle in the viewport.
func center(ctx context.Context, t *chromedp.Target, node *chromedp.Node) (float64, float64, error) {
	if _, err := cdp.Call(ctx, t, dom.ScrollIntoViewIfNeeded, dom.ScrollIntoViewIfNeededParams{NodeID: node.NodeID}); err != nil {
		return 0, 0, err
	}
	quads, err := cdp.Call(ctx, t, dom.GetContentQuads, dom.GetContentQuadsParams{NodeID: node.NodeID})
	if err != nil {
		return 0, 0, err
	}
	if len(quads.Quads) == 0 || len(quads.Quads[0]) != 8 {
		return 0, 0, errors.New("element has no visible box")
	}
	quad := quads.Quads[0]
	return (quad[0] + quad[2] + quad[4] + quad[6]) / 4, (quad[1] + quad[3] + quad[5] + quad[7]) / 4, nil
}

const describeElement = `function(limit){return this.tagName.toLowerCase()+': '+String(this.innerText||this.value||'').trim().slice(0,limit)}`

const pageContext = `function(){return {url:location.href,title:document.title,page_text:String(document.body?document.body.innerText:'').slice(0,2000)}}`

// settle gives an action that may navigate time to finish loading.
func (session *session) settle(ctx context.Context) {
	bound, cancel, err := session.top(ctx, 5*time.Second)
	if err != nil {
		return
	}
	defer cancel()
	_, _ = chromedp.Run(bound, chromedp.Sleep(100*time.Millisecond))
	_, _ = chromedp.Run(bound, chromedp.PollFunction[bool]("() => document.readyState === 'complete'", chromedp.WithPollingTimeout(5*time.Second)))
}

// context describes the page for the model after an action.
func (session *session) context(ctx context.Context) any {
	bound, cancel, err := session.top(ctx, 5*time.Second)
	if err != nil {
		return map[string]any{}
	}
	defer cancel()
	value, err := chromedp.Run(bound, chromedp.Evaluate[map[string]any]("("+pageContext+")()"))
	if err != nil || value == nil {
		return map[string]any{}
	}
	return value
}

func (session *session) location(ctx context.Context) (string, string, error) {
	bound, cancel, err := session.top(ctx, 5*time.Second)
	if err != nil {
		return "", "", err
	}
	defer cancel()
	address, err := chromedp.Run(bound, chromedp.Location())
	if err != nil {
		return "", "", err
	}
	title, err := chromedp.Run(bound, chromedp.Title())
	return address, title, err
}

func navigate(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	address, err := required(arguments, "url")
	if err != nil {
		return nil, err
	}
	return history(chromedp.Navigate(address), "Navigation failed")(ctx, session, arguments)
}

func history(action chromedp.Action[chromedp.Void], prefix string) operation {
	return func(ctx context.Context, session *session, _ map[string]any) (map[string]any, error) {
		bound, cancel, err := session.top(ctx, pageTimeout)
		if err != nil {
			return nil, err
		}
		defer cancel()
		session.current.leave(0)
		if err := chromedp.Do(bound, action); err != nil {
			return nil, failed(prefix, err)
		}
		state := session.context(ctx)
		values, _ := state.(map[string]any)
		return map[string]any{"success": true, "url": values["url"], "title": values["title"], "page": state}, nil
	}
}

func click(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	selector, err := required(arguments, "selector")
	if err != nil {
		return nil, err
	}
	limit := waitFor(arguments, "wait_timeout", 10)
	var described any
	err = session.element(ctx, selector, limit, true, func(ctx context.Context, t *chromedp.Target, node *chromedp.Node) error {
		described, _ = callOn(ctx, t, node, describeElement, 50)
		_, err := chromedp.MouseClickNode(node)(ctx, t)
		return err
	})
	if err != nil {
		return nil, failed("Click failed", err)
	}
	session.settle(ctx)
	return map[string]any{"success": true, "clicked_element": described, "page": session.context(ctx)}, nil
}

func doubleClick(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	selector, err := required(arguments, "selector")
	if err != nil {
		return nil, err
	}
	err = session.element(ctx, selector, 0, true, func(ctx context.Context, t *chromedp.Target, node *chromedp.Node) error {
		_, err := chromedp.MouseClickNode(node, chromedp.ClickCount(2))(ctx, t)
		return err
	})
	if err != nil {
		return nil, failed("Double click failed", err)
	}
	session.settle(ctx)
	return map[string]any{"success": true, "double_clicked_element": selector}, nil
}

func rightClick(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	selector, err := required(arguments, "selector")
	if err != nil {
		return nil, err
	}
	err = session.element(ctx, selector, 0, true, func(ctx context.Context, t *chromedp.Target, node *chromedp.Node) error {
		_, err := chromedp.MouseClickNode(node, chromedp.ButtonRight)(ctx, t)
		return err
	})
	if err != nil {
		return nil, failed("Right click failed", err)
	}
	return map[string]any{"success": true, "right_clicked": selector}, nil
}

func hover(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	selector, err := required(arguments, "selector")
	if err != nil {
		return nil, err
	}
	err = session.element(ctx, selector, 0, true, func(ctx context.Context, t *chromedp.Target, node *chromedp.Node) error {
		x, y, err := center(ctx, t, node)
		if err != nil {
			return err
		}
		_, err = chromedp.MouseEvent(chromedp.MouseMoved, x, y)(ctx, t)
		return err
	})
	if err != nil {
		return nil, failed("Hover failed", err)
	}
	return map[string]any{"success": true, "hovered": selector}, nil
}

func focus(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	selector, err := required(arguments, "selector")
	if err != nil {
		return nil, err
	}
	err = session.element(ctx, selector, 0, false, func(ctx context.Context, t *chromedp.Target, node *chromedp.Node) error {
		_, err := cdp.Call(ctx, t, dom.Focus, dom.FocusParams{NodeID: node.NodeID})
		return err
	})
	if err != nil {
		return nil, failed("Focus failed", err)
	}
	return map[string]any{"success": true, "focused": selector}, nil
}

const clearElement = `function(){this.focus();if('value' in this){this.value='';}else if(this.isContentEditable){this.textContent='';}this.dispatchEvent(new Event('input',{bubbles:true}));this.dispatchEvent(new Event('change',{bubbles:true}));}`

func typeText(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	selector, err := required(arguments, "selector")
	if err != nil {
		return nil, err
	}
	text, err := required(arguments, "text")
	if err != nil {
		return nil, err
	}
	var value any
	err = session.element(ctx, selector, 10*time.Second, true, func(ctx context.Context, t *chromedp.Target, node *chromedp.Node) error {
		if boolean(arguments, "clear_first", true) {
			if _, err := callOn(ctx, t, node, clearElement); err != nil {
				return fmt.Errorf("Clear failed: %w", err)
			}
		}
		if _, err := chromedp.KeyEventNode(node, text)(ctx, t); err != nil {
			return err
		}
		value, _ = callOn(ctx, t, node, `function(){return this.value}`)
		if boolean(arguments, "press_enter", false) {
			if _, err := chromedp.KeyEvent(kb.Enter)(ctx, t); err != nil {
				return fmt.Errorf("Enter key failed: %w", err)
			}
		}
		return nil
	})
	if err != nil {
		return nil, failed("Type failed", err)
	}
	if boolean(arguments, "press_enter", false) {
		session.settle(ctx)
	}
	if _, ok := value.(string); !ok {
		value = text
	}
	return map[string]any{"success": true, "typed_text": text, "field_value": value, "page": session.context(ctx)}, nil
}

func clearField(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	selector, err := required(arguments, "selector")
	if err != nil {
		return nil, err
	}
	err = session.element(ctx, selector, 0, false, func(ctx context.Context, t *chromedp.Target, node *chromedp.Node) error {
		_, err := callOn(ctx, t, node, clearElement)
		return err
	})
	if err != nil {
		return nil, failed("Clear failed", err)
	}
	return map[string]any{"success": true, "cleared": selector, "page": session.context(ctx)}, nil
}

const chooseOption = `function(a){
const s=document.querySelector(a.selector);
if(!s||!s.options)return {missing:true};
const o=Array.from(s.options);
let i=-1;
if(typeof a.value==='string')i=o.findIndex(x=>x.value===a.value);
else if(typeof a.text==='string')i=o.findIndex(x=>x.text===a.text);
else if(a.index<o.length)i=a.index;
if(i<0)return {absent:true};
s.selectedIndex=i;
s.dispatchEvent(new Event('input',{bubbles:true}));
s.dispatchEvent(new Event('change',{bubbles:true}));
return {text:o[i].text};
}`

func selectOption(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	selector, err := required(arguments, "selector")
	if err != nil {
		return nil, err
	}
	value, byValue := argument[string](arguments, "value")
	text, byText := argument[string](arguments, "text")
	index, byIndex := argument[float64](arguments, "index")
	if !byValue && !byText && !byIndex {
		return nil, unavailable("Must specify 'value', 'text', or 'index'")
	}
	chosen, err := session.call(ctx, scriptTimeout, chooseOption, arguments)
	if err != nil {
		return nil, failed("Select failed", err)
	}
	outcome, _ := chosen.(map[string]any)
	if outcome["missing"] == true {
		return nil, unavailable("Element not found: " + selector)
	}
	session.settle(ctx)
	state := session.context(ctx)
	switch {
	case byValue:
		if outcome["absent"] == true {
			return nil, unavailable("Element not found: " + selector + " option[value='" + value + "']")
		}
		return map[string]any{"success": true, "selected_value": value, "page": state}, nil
	case byText:
		if outcome["absent"] == true {
			return nil, unavailable("Option with text '" + text + "' not found")
		}
		return map[string]any{"success": true, "selected_text": text, "page": state}, nil
	}
	if outcome["absent"] == true {
		return nil, unavailable(fmt.Sprintf("Option at index %v not found", index))
	}
	return map[string]any{"success": true, "selected_text": outcome["text"], "selected_index": index, "page": state}, nil
}

var namedKeys = map[string]string{
	"enter": kb.Enter, "tab": kb.Tab, "escape": kb.Escape, "esc": kb.Escape, "backspace": kb.Backspace, "delete": kb.Delete,
	"arrowup": kb.ArrowUp, "up": kb.ArrowUp, "arrowdown": kb.ArrowDown, "down": kb.ArrowDown,
	"arrowleft": kb.ArrowLeft, "left": kb.ArrowLeft, "arrowright": kb.ArrowRight, "right": kb.ArrowRight,
	"home": kb.Home, "end": kb.End, "pageup": kb.PageUp, "pagedown": kb.PageDown, "space": " ",
}

func pressKey(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	key, err := required(arguments, "key")
	if err != nil {
		return nil, err
	}
	keys := key
	if named, ok := namedKeys[strings.ToLower(key)]; ok {
		keys = named
	}
	names := textList(arguments, "modifiers")
	var modifiers []chromedp.Modifier
	for _, name := range names {
		switch strings.ToLower(name) {
		case "ctrl", "control":
			modifiers = append(modifiers, chromedp.ModifierCtrl)
		case "alt":
			modifiers = append(modifiers, chromedp.ModifierAlt)
		case "shift":
			modifiers = append(modifiers, chromedp.ModifierShift)
		case "meta", "command", "cmd":
			modifiers = append(modifiers, chromedp.ModifierMeta)
		}
	}
	selector, targeted := argument[string](arguments, "selector")
	if targeted {
		err = session.element(ctx, selector, 0, false, func(ctx context.Context, t *chromedp.Target, node *chromedp.Node) error {
			_, err := chromedp.KeyEventNode(node, keys, chromedp.KeyModifiers(modifiers...))(ctx, t)
			return err
		})
	} else {
		var target context.Context
		if target, _, err = session.scope(); err == nil {
			bound, cancel := bind(ctx, target, scriptTimeout)
			err = chromedp.Do(bound, chromedp.KeyEvent(keys, chromedp.KeyModifiers(modifiers...)))
			cancel()
		}
	}
	if err != nil {
		return nil, failed("Key press failed", err)
	}
	session.settle(ctx)
	result := map[string]any{"success": true, "key_pressed": key, "modifiers": names, "target": nil}
	if targeted {
		result["target"] = selector
	}
	return result, nil
}

func dragAndDrop(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	from, err := required(arguments, "source_selector")
	if err != nil {
		return nil, err
	}
	to, err := required(arguments, "target_selector")
	if err != nil {
		return nil, err
	}
	target, owner, err := session.scope()
	if err != nil {
		return nil, err
	}
	bound, cancel := bind(ctx, target, 10*time.Second)
	defer cancel()
	if err := chromedp.Do(bound, chromedp.DragAndDrop(chromedp.CSS(from), chromedp.CSS(to), within(owner)...)); err != nil {
		return nil, failed("Drag and drop failed", err)
	}
	return map[string]any{"success": true, "dragged_from": from, "dropped_on": to}, nil
}

func fileUpload(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	selector, err := required(arguments, "selector")
	if err != nil {
		return nil, err
	}
	file, err := required(arguments, "file_path")
	if err != nil {
		return nil, err
	}
	err = session.element(ctx, selector, 0, false, func(ctx context.Context, t *chromedp.Target, node *chromedp.Node) error {
		_, err := cdp.Call(ctx, t, dom.SetFileInputFiles, dom.SetFileInputFilesParams{Files: []string{file}, NodeID: node.NodeID})
		return err
	})
	if err != nil {
		return nil, failed("File upload failed", err)
	}
	return map[string]any{"success": true, "uploaded_file": file, "to_element": selector}, nil
}

const scrollPage = `function(a){
if(typeof a.selector==='string'){const e=document.querySelector(a.selector);if(!e)return {missing:true};e.scrollIntoView({block:'center'});return {};}
if(a.direction==='up')window.scrollBy(0,-a.amount);
else if(a.direction==='down')window.scrollBy(0,a.amount);
else if(a.direction==='top')window.scrollTo(0,0);
else window.scrollTo(0,document.body.scrollHeight);
return {};
}`

func scroll(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	selector, bySelector := argument[string](arguments, "selector")
	direction, byDirection := argument[string](arguments, "direction")
	if !bySelector && !byDirection {
		return nil, unavailable("Must specify either 'direction' or 'selector'")
	}
	if !bySelector && direction != "up" && direction != "down" && direction != "top" && direction != "bottom" {
		return nil, unavailable("Invalid direction: " + direction)
	}
	moved, err := session.call(ctx, scriptTimeout, scrollPage, map[string]any{"selector": arguments["selector"], "direction": direction, "amount": integer(arguments, "amount", 500)})
	if err != nil {
		return nil, failed("Scroll failed", err)
	}
	if outcome, _ := moved.(map[string]any); outcome["missing"] == true {
		return nil, unavailable("Element not found: " + selector)
	}
	if bySelector {
		return map[string]any{"success": true, "scrolled_to": selector}, nil
	}
	return map[string]any{"success": true, "scrolled": direction}, nil
}

// inspect runs a read-only script and maps its missing marker to a fault.
func (session *session) inspect(ctx context.Context, function string, arguments map[string]any, selector string) (map[string]any, error) {
	value, err := session.call(ctx, scriptTimeout, function, arguments)
	if err != nil {
		return nil, failed("Element lookup failed", err)
	}
	result, _ := value.(map[string]any)
	if result == nil || result["missing"] == true {
		return nil, unavailable("Element not found: " + selector)
	}
	return result, nil
}

const readText = `function(a){
const text=e=>String(e.innerText??e.textContent??'').trim();
if(a.all===true){const t=Array.from(document.querySelectorAll(a.selector)).map(text);return {success:true,texts:t,count:t.length};}
const e=document.querySelector(a.selector);
return e?{success:true,text:text(e)}:{missing:true};
}`

func extractText(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	selector, err := required(arguments, "selector")
	if err != nil {
		return nil, err
	}
	return session.inspect(ctx, readText, arguments, selector)
}

const readAttribute = `function(a){
const e=document.querySelector(a.selector);
return e?{success:true,attribute:a.attribute,value:e.getAttribute(a.attribute)}:{missing:true};
}`

func extractAttribute(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	selector, err := required(arguments, "selector")
	if err != nil {
		return nil, err
	}
	if _, err := required(arguments, "attribute"); err != nil {
		return nil, err
	}
	return session.inspect(ctx, readAttribute, arguments, selector)
}

const readLinks = `function(a){
const scope=typeof a.selector==='string'?a.selector+' a[href]':'a[href]';
const links=Array.from(document.querySelectorAll(scope)).map(e=>({href:e.getAttribute('href'),text:a.include_text===false?null:String(e.innerText||'').trim()}));
return {success:true,links:links,count:links.length};
}`

func extractLinks(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	return session.inspect(ctx, readLinks, arguments, "a[href]")
}

const readState = `function(a){
const e=document.querySelector(a.selector);
if(!e)return {missing:true};
const r=e.getBoundingClientRect(),s=getComputedStyle(e);
const shown=r.width>0&&r.height>0&&s.visibility!=='hidden'&&s.display!=='none';
const enabled=!e.disabled;
return {success:true,selector:a.selector,is_displayed:shown,is_enabled:enabled,is_selected:!!(e.checked||e.selected),is_clickable:shown&&enabled};
}`

func elementState(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	selector, err := required(arguments, "selector")
	if err != nil {
		return nil, err
	}
	return session.inspect(ctx, readState, arguments, selector)
}

func pageInfo(ctx context.Context, session *session, _ map[string]any) (map[string]any, error) {
	address, title, err := session.location(ctx)
	if err != nil {
		return nil, failed("Failed to read the page", err)
	}
	return map[string]any{"success": true, "url": address, "title": title}, nil
}

func pageSource(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	limit := int(max(integer(arguments, "max_length", 50000), 0))
	value, err := session.call(ctx, scriptTimeout, `function(){return document.documentElement?document.documentElement.outerHTML:''}`, nil)
	if err != nil {
		return nil, failed("Failed to read the page source", err)
	}
	source, _ := value.(string)
	html := source
	if len(source) > limit {
		// The limit counts characters; a multi-byte page keeps more bytes.
		end := 0
		for count := 0; count < limit && end < len(source); count++ {
			_, size := utf8.DecodeRuneInString(source[end:])
			end += size
		}
		html = source[:end]
	}
	return map[string]any{"success": true, "html": html, "truncated": len(source) > limit, "total_length": len(source), "returned_length": len(html)}, nil
}

// evaluate keeps the WebDriver script contract the tool documents: the
// script is a function body, and an async script receives its completion
// callback as the last argument.
func evaluate(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	script, err := required(arguments, "script")
	if err != nil {
		return nil, err
	}
	function, prefix := "function(){return (function(){"+script+"\n}).call(window)}", "Script execution failed"
	if boolean(arguments, "async", false) {
		function, prefix = "function(){return new Promise(function(done){(function(){"+script+"\n}).call(window,done)})}", "Async script failed"
	}
	value, err := session.call(ctx, scriptTimeout, function, nil)
	if err != nil {
		if ctx.Err() == nil && errors.Is(err, context.DeadlineExceeded) {
			return nil, unavailable(prefix + ": script timeout")
		}
		return nil, failed(prefix, err)
	}
	return map[string]any{"success": true, "result": value}, nil
}

func screenshot(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	var image []byte
	var err error
	if selector, ok := argument[string](arguments, "selector"); ok {
		err = session.element(ctx, selector, 0, true, func(ctx context.Context, t *chromedp.Target, node *chromedp.Node) error {
			image, err = chromedp.ScreenshotNodes([]*chromedp.Node{node}, 1)(ctx, t)
			return err
		})
	} else {
		var bound context.Context
		var cancel context.CancelFunc
		if bound, cancel, err = session.top(ctx, scriptTimeout); err == nil {
			image, err = chromedp.Run(bound, chromedp.CaptureScreenshot())
			cancel()
		}
	}
	if err != nil {
		return nil, failed("Screenshot failed", err)
	}
	return map[string]any{"success": true, "base64_image": base64.StdEncoding.EncodeToString(image), "saved_to_artifacts": false, "artifact_name": nil}, nil
}

func printToPDF(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	bound, cancel, err := session.top(ctx, pageTimeout)
	if err != nil {
		return nil, err
	}
	defer cancel()
	options := []chromedp.PDFOption{}
	if boolean(arguments, "landscape", false) {
		options = append(options, chromedp.PDFLandscape())
	}
	if scale, ok := argument[float64](arguments, "scale"); ok {
		options = append(options, chromedp.PDFScale(scale))
	}
	document, err := chromedp.Run(bound, chromedp.PrintToPDF(options...))
	if err != nil {
		return nil, failed("Print to PDF failed", err)
	}
	return map[string]any{"success": true, "pdf_base64": base64.StdEncoding.EncodeToString(document)}, nil
}

// handleAlert sets how this session answers JavaScript dialogs. A dialog
// that already opened was answered with the previous choice, because the
// page cannot run again until its dialog is answered.
func handleAlert(_ context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	action, err := required(arguments, "action")
	if err != nil {
		return nil, err
	}
	if action != "accept" && action != "dismiss" {
		return nil, unavailable("Invalid action: " + action)
	}
	text, _ := argument[string](arguments, "text")
	session.mu.Lock()
	seen := session.dialog.seen
	session.dialog = dialogs{accept: action == "accept", text: text}
	session.mu.Unlock()
	return map[string]any{"success": true, "action": action, "had_active_alert": seen}, nil
}

// waitFor is how long a call waits for a page condition: the seconds the
// model asked for, at most waitLimit. A longer wait would run into the call
// deadline, which closes the browser instead of answering.
func waitFor(arguments map[string]any, name string, fallback int64) time.Duration {
	seconds := min(max(integer(arguments, name, fallback), 0), int64(waitLimit/time.Second))
	return time.Duration(seconds) * time.Second
}

// elapsed names a wait that ended without its condition.
func elapsed(limit time.Duration) string {
	return fmt.Sprintf(" after %ds", int(limit/time.Second))
}

func wait(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	seconds, _ := argument[float64](arguments, "seconds")
	duration, err := waitDuration(seconds)
	if err != nil {
		return nil, err
	}
	if duration > waitLimit {
		// The result reports the time that actually passed.
		duration, seconds = waitLimit, waitLimit.Seconds()
	}
	select {
	case <-session.clock.After(duration):
	case <-ctx.Done():
		return nil, ctx.Err()
	}
	return map[string]any{"success": true, "waited_seconds": seconds}, nil
}

func waitForElement(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	selector, err := required(arguments, "selector")
	if err != nil {
		return nil, err
	}
	limit := waitFor(arguments, "timeout", 30)
	var described any
	err = session.element(ctx, selector, limit, boolean(arguments, "visible", false), func(ctx context.Context, t *chromedp.Target, node *chromedp.Node) error {
		described, _ = callOn(ctx, t, node, `function(){return String(this.innerText||'').trim().slice(0,100)}`)
		return nil
	})
	if errors.Is(err, errNotFound) {
		return nil, unavailable("Element not found" + elapsed(max(limit, time.Second)) + ": " + selector)
	}
	if err != nil {
		return nil, failed("Wait failed", err)
	}
	return map[string]any{"success": true, "found": true, "element_text": described}, nil
}

func waitForPageLoad(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	limit := waitFor(arguments, "timeout", 30)
	bound, cancel, err := session.top(ctx, limit+time.Second)
	if err != nil {
		return nil, err
	}
	defer cancel()
	if _, err := chromedp.Run(bound, chromedp.PollFunction[bool]("() => document.readyState === 'complete'", chromedp.WithPollingTimeout(limit))); err != nil {
		if waited(ctx, err) {
			return nil, unavailable("Page load timeout" + elapsed(limit))
		}
		return nil, failed("Page load wait failed", err)
	}
	address, title, err := session.location(ctx)
	if err != nil {
		return nil, failed("Failed to read the page", err)
	}
	return map[string]any{"success": true, "url": address, "title": title, "ready_state": "complete"}, nil
}

func waitForText(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	text, err := required(arguments, "text")
	if err != nil {
		return nil, err
	}
	limit := waitFor(arguments, "timeout", 30)
	target, owner, err := session.scope()
	if err != nil {
		return nil, err
	}
	bound, cancel := bind(ctx, target, limit+time.Second)
	defer cancel()
	options := []chromedp.PollOption{chromedp.WithPollingArgs(text), chromedp.WithPollingTimeout(limit)}
	if owner != nil {
		options = append(options, chromedp.WithPollingInFrame(owner))
	}
	if _, err := chromedp.Run(bound, chromedp.PollFunction[bool]("(text) => !!document.body && document.body.innerText.includes(text)", options...)); err != nil {
		if waited(ctx, err) {
			return nil, unavailable("Text '" + text + "' not found" + elapsed(limit))
		}
		return nil, failed("Text '"+text+"' not found", err)
	}
	return map[string]any{"success": true, "found": true, "text": text}, nil
}

func listWindows(ctx context.Context, session *session, _ map[string]any) (map[string]any, error) {
	bound, cancel := bind(ctx, session.ctx, scriptTimeout)
	defer cancel()
	infos, err := session.pages(bound)
	if err != nil {
		return nil, failed("Failed to get windows", err)
	}
	handles := make([]string, 0, len(infos))
	for _, info := range infos {
		handles = append(handles, string(info.TargetID))
	}
	var current any
	if session.current != nil {
		current = string(session.current.id)
	}
	return map[string]any{"success": true, "windows": handles, "current_window": current, "count": len(handles)}, nil
}

func newWindow(separate bool) operation {
	return func(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
		opened, err := session.open(separate)
		if err != nil {
			return nil, failed("Failed to open new tab", err)
		}
		if address, ok := argument[string](arguments, "url"); ok {
			bound, cancel := bind(ctx, opened.ctx, pageTimeout)
			err := chromedp.Do(bound, chromedp.Navigate(address))
			cancel()
			if err != nil {
				return nil, failed("Navigation failed", err)
			}
		}
		address, _, err := session.location(ctx)
		if err != nil {
			return nil, failed("Failed to read the page", err)
		}
		return map[string]any{"success": true, "window_handle": string(opened.id), "url": address}, nil
	}
}

func switchWindow(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	handle, err := required(arguments, "handle")
	if err != nil {
		return nil, err
	}
	bound, cancel := bind(ctx, session.ctx, scriptTimeout)
	err = session.switchTo(bound, handle)
	cancel()
	if err != nil {
		return nil, failed("Failed to switch window", err)
	}
	if front, cancel, err := session.top(ctx, scriptTimeout); err == nil {
		_, _ = chromedp.Call(front, page.BringToFront, cdp.Empty{})
		cancel()
	}
	address, title, err := session.location(ctx)
	if err != nil {
		return nil, failed("Failed to read the page", err)
	}
	return map[string]any{"success": true, "switched_to": handle, "url": address, "title": title}, nil
}

func closeWindow(ctx context.Context, session *session, _ map[string]any) (map[string]any, error) {
	closing := session.current
	if closing == nil {
		return nil, unavailable("no browser window is open; open a new tab")
	}
	bound, cancel := bind(ctx, session.ctx, scriptTimeout)
	defer cancel()
	if _, err := chromedp.CallBrowser(bound, target.CloseTarget, target.CloseTargetParams{TargetID: closing.id}); err != nil {
		return nil, failed("Failed to close window", err)
	}
	closing.leave(0)
	closing.cancel()
	delete(session.windows, closing.id)
	session.current = nil
	// The next remaining page becomes current, as a user would see it.
	if infos, err := session.pages(bound); err == nil {
		for _, info := range infos {
			if info.TargetID != closing.id && session.switchTo(bound, string(info.TargetID)) == nil {
				break
			}
		}
	}
	return map[string]any{"success": true, "message": "Window closed"}, nil
}

func (session *session) bounds(ctx context.Context, bounds browserBounds) error {
	if session.current == nil {
		return unavailable("no browser window is open; open a new tab")
	}
	bound, cancel := bind(ctx, session.ctx, scriptTimeout)
	defer cancel()
	return setBounds(bound, session.current.id, bounds)
}

func windowState(state, message string) operation {
	return func(ctx context.Context, session *session, _ map[string]any) (map[string]any, error) {
		if err := session.bounds(ctx, browserBounds{state: state}); err != nil {
			return nil, failed("Failed to change the window", err)
		}
		return map[string]any{"success": true, "message": message}, nil
	}
}

func setWindowSize(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	width, height := integer(arguments, "width", -1), integer(arguments, "height", -1)
	if width < 0 {
		return nil, unavailable("Missing 'width' parameter")
	}
	if height < 0 {
		return nil, unavailable("Missing 'height' parameter")
	}
	x, y := integer(arguments, "x", 0), integer(arguments, "y", 0)
	if err := session.bounds(ctx, browserBounds{state: "normal"}); err != nil {
		return nil, failed("Failed to set window size", err)
	}
	if err := session.bounds(ctx, browserBounds{width: width, height: height, x: &x, y: &y}); err != nil {
		return nil, failed("Failed to set window size", err)
	}
	return map[string]any{"success": true, "width": width, "height": height, "x": x, "y": y}, nil
}

func switchToFrame(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	index, byIndex := argument[float64](arguments, "index")
	selector, bySelector := argument[string](arguments, "selector")
	if !byIndex && !bySelector {
		return nil, unavailable("Must provide either 'index' or 'selector'")
	}
	scope, owner, err := session.scope()
	if err != nil {
		return nil, err
	}
	lookup, position := selector, 0
	if byIndex {
		lookup, position = "iframe, frame", int(index)
	}
	bound, cancel := bind(ctx, scope, 2*time.Second)
	nodes, err := chromedp.Run(bound, chromedp.Nodes(chromedp.CSSAll(lookup), within(owner, chromedp.AtLeast(position+1))...))
	cancel()
	if err != nil || position < 0 || position >= len(nodes) {
		if byIndex {
			return nil, unavailable(fmt.Sprintf("Failed to switch to frame %v: no such frame", index))
		}
		return nil, unavailable("Failed to switch to frame: Element not found: " + selector)
	}
	element := nodes[position]
	switch {
	case element.ContentDocument != nil:
		session.current.frames = append(session.current.frames, frame{ctx: scope, owner: element})
	case element.FrameID != "":
		// A frame from another site renders in its own process and target.
		attached, detach := chromedp.NewContext(session.ctx, chromedp.WithTargetID(target.ID(element.FrameID)), chromedp.WithDetachOnCancel())
		// The attachment outlives this call, so it starts without a deadline;
		// an interrupted call closes the session, which ends it.
		if err := chromedp.Do(attached); err != nil {
			detach()
			return nil, failed("Failed to switch to frame", err)
		}
		session.current.frames = append(session.current.frames, frame{ctx: attached, cancel: detach})
	default:
		return nil, unavailable("Failed to switch to frame: element is not a frame")
	}
	if byIndex {
		return map[string]any{"success": true, "switched_to_frame": index}, nil
	}
	return map[string]any{"success": true, "switched_to_frame": selector}, nil
}

func switchToParentFrame(_ context.Context, session *session, _ map[string]any) (map[string]any, error) {
	if session.current == nil {
		return nil, unavailable("no browser window is open; open a new tab")
	}
	if depth := len(session.current.frames); depth > 0 {
		session.current.leave(depth - 1)
	}
	return map[string]any{"success": true, "message": "Switched to parent frame"}, nil
}

func switchToDefaultContent(_ context.Context, session *session, _ map[string]any) (map[string]any, error) {
	if session.current == nil {
		return nil, unavailable("no browser window is open; open a new tab")
	}
	session.current.leave(0)
	return map[string]any{"success": true, "message": "Switched to default content"}, nil
}
