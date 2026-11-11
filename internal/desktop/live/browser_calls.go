package live

import (
	"context"
	_ "embed"
	"encoding/json"
	"errors"
	"slices"
	"strconv"
	"time"

	"github.com/egoist/mygo"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// browserAutomation is invoked only by the typed browser adapter. Source the
// model provides is never evaluated.
//
//go:embed browser_automation.js
var browserAutomation string

// browserRequest is the part of an action the adapter decides on; the whole
// action goes to the page script as it is.
type browserRequest struct {
	Action     string          `json:"action"`
	Tab        *uint64         `json:"tab"`
	URL        string          `json:"url"`
	Background bool            `json:"background"`
	TimeoutMs  uint32          `json:"timeout_ms"`
	Target     json.RawMessage `json:"target"`
	Scale      string          `json:"scale"`
	Format     string          `json:"format"`
}

// browserPlan is what the main thread decided for a call: its settled result,
// or the tab and page the adapter goes on with.
type browserPlan struct {
	result *protocol.BrowserResult
	tab    *browserTab
	view   *mygo.WebView
}

func browserFailure(code protocol.ErrorCode, message string) protocol.BrowserResult {
	return protocol.BrowserResult{Err: &protocol.Fault{Code: code, Message: message}}
}

func browserUnavailable() protocol.BrowserResult {
	return browserFailure(protocol.ErrorCodeUnavailable, "browser page is unavailable")
}

func browserValue(value any) protocol.BrowserResult {
	encoded, err := json.Marshal(value)
	if err != nil {
		return browserUnavailable()
	}
	raw := json.RawMessage(encoded)
	return protocol.BrowserResult{Ok: &raw}
}

// browserMain runs work on MyGo's main thread, where the panel and the
// WebViews live, and gives up when ctx ends: a closing window waits for its
// workers on that thread.
func browserMain[T any](ctx context.Context, work func() T) (T, bool) {
	done := make(chan T, 1)
	go mygo.RunOnMain(func() { browserQueued(ctx, done, work) })
	select {
	case value := <-done:
		return value, true
	case <-ctx.Done():
		var zero T
		return zero, false
	}
}

// browserQueued is a queued action's turn on the main thread. An action
// whose call ended while it waited there does not run: the Node has given
// the call up and the agent may already be acting on its answer.
func browserQueued[T any](ctx context.Context, done chan<- T, work func() T) {
	if ctx.Err() == nil {
		done <- work()
	}
}

// browserEnded is the result of a call cut short: by its own deadline, or by
// the browser's service closing.
func browserEnded(ctx context.Context) protocol.BrowserResult {
	if errors.Is(ctx.Err(), context.DeadlineExceeded) {
		return browserFailure(protocol.ErrorCodeCancelled, "browser call expired")
	}
	return browserFailure(protocol.ErrorCodeCancelled, "browser is closed")
}

func browserSleep(ctx context.Context, duration time.Duration) bool {
	timer := time.NewTimer(duration)
	defer timer.Stop()
	select {
	case <-timer.C:
		return true
	case <-ctx.Done():
		return false
	}
}

// executeBrowserCall runs one of the agent's browser actions against the
// tabs of its session. The WebView is the execution adapter; authorization
// lives on the Node. ctx ends with the call (ServeBrowser), which bounds
// every step and a wait whose own timeout is longer.
func (r *Runtime) executeBrowserCall(ctx context.Context, owner *Runtime, call protocol.BrowserCall) protocol.BrowserResult {
	if uint64(time.Now().UnixMilli()) >= call.ExpiresAtMs || ctx.Err() != nil {
		return browserFailure(protocol.ErrorCodeCancelled, "browser call expired")
	}
	action, err := json.Marshal(call.Action)
	var request browserRequest
	if err != nil || json.Unmarshal(action, &request) != nil {
		return browserFailure(protocol.ErrorCodeInvalidRequest, "invalid browser operation")
	}
	plan, ran := browserMain(ctx, func() browserPlan {
		plan := r.planBrowserCall(owner.controller.Target(), call.Session.String(), request)
		r.wake()
		return plan
	})
	if !ran {
		return browserEnded(ctx)
	}
	if plan.result != nil {
		return *plan.result
	}
	if request.Action == "screenshot" {
		return r.captureBrowser(ctx, plan, request)
	}
	deadline := time.Now().Add(20 * time.Second)
	if request.Action == "wait" {
		deadline = time.Now().Add(time.Duration(request.TimeoutMs) * time.Millisecond)
	}
	switch request.Action {
	case "navigate", "open", "read", "focus", "wait":
	default:
		if _, failure := browserEvaluate(ctx, plan.view, action); failure != nil {
			return *failure
		}
		// Let the web engine publish a navigation started by a click or by
		// going back before the document is inspected.
		if !browserSleep(ctx, 100*time.Millisecond) {
			return browserEnded(ctx)
		}
	}
	if failure := browserLoaded(ctx, plan, deadline); failure != nil {
		return *failure
	}
	for request.Action == "wait" {
		value, failure := browserEvaluate(ctx, plan.view, action)
		if failure != nil {
			return *failure
		}
		if ready, _ := value["ready"].(bool); ready {
			break
		}
		if !time.Now().Before(deadline) {
			return browserFailure(protocol.ErrorCodeUnavailable, "browser wait timed out")
		}
		if !browserSleep(ctx, 100*time.Millisecond) {
			return browserEnded(ctx)
		}
	}
	read, _ := json.Marshal(map[string]string{"action": "read", "snapshot": protocol.NewRequestID().String()})
	value, failure := browserEvaluate(ctx, plan.view, read)
	if failure != nil {
		return *failure
	}
	value["tab"] = plan.tab.id
	return browserValue(value)
}

// planBrowserCall addresses the stable tab IDs of one session's tabs. Main
// thread only.
func (r *Runtime) planBrowserCall(node protocol.NodeID, session string, request browserRequest) browserPlan {
	settled := func(result protocol.BrowserResult) browserPlan { return browserPlan{result: &result} }
	state := r.browser
	if state == nil || !state.available {
		return settled(browserUnavailable())
	}
	selected := r.App.Sidebar.Selected == session
	var tabs []*browserTab
	// The session's tabs are in its own panel, shown or not.
	for _, open := range r.App.AllPanelTabs() {
		if open.Kind != panel.Browser {
			continue
		}
		tab := state.tabs[open.ID]
		if tab == nil {
			// A tab that showed no page yet, as one that came back with a
			// conversation not shown since, is its panel's conversation's:
			// this session's in its own panel, and never the selected one's
			// for being selected.
			if !slices.Contains(r.App.ConversationTabs(session), open) {
				continue
			}
			if tab = r.browserTabFor(open.ID); tab == nil {
				continue
			}
			tab.node, tab.conversation = node, session
		}
		if tab.node != node {
			continue
		}
		// A tab opened in the draft that became this session is already its
		// own (browserAdopt); another conversation's tab never is.
		if tab.conversation == session {
			tabs = append(tabs, tab)
		}
	}
	add := func(show bool) *browserTab {
		open := r.App.Panel.AddBrowser(show && selected)
		tab := r.browserTabFor(open.ID)
		tab.node, tab.conversation = node, session
		r.App.MovePanelTab(open, session)
		tabs = append(tabs, tab)
		return tab
	}
	if request.Action == "tabs" {
		listed := []map[string]any{}
		for _, tab := range tabs {
			loading := tab.view != nil && !tab.view.IsDestroyed() && tab.view.Page().IsLoading()
			listed = append(listed, map[string]any{"id": tab.id, "url": tab.url, "title": tab.title, "loading": loading, "background": tab.background})
		}
		return settled(browserValue(map[string]any{"tabs": listed}))
	}
	if (request.Action == "navigate" || request.Action == "open") && !browserAddress(request.URL) {
		return settled(browserFailure(protocol.ErrorCodeInvalidRequest, "browser URL must use HTTP or HTTPS without embedded credentials"))
	}
	if request.Action == "open" {
		tab := add(!request.Background)
		request.Action, request.Tab = "navigate", &tab.id
	}
	if len(tabs) == 0 {
		add(false)
	}
	// Without a tab named, the one the session's own panel shows.
	target := tabs[0]
	if active := r.App.ConversationActiveTab(session); active.Kind == panel.Browser {
		for _, tab := range tabs {
			if strconv.FormatUint(tab.id, 10) == active.ID {
				target = tab
			}
		}
	}
	if request.Tab != nil {
		target = nil
		for _, tab := range tabs {
			if tab.id == *request.Tab {
				target = tab
			}
		}
		if target == nil {
			return settled(browserFailure(protocol.ErrorCodeNotFound, "browser tab is closed"))
		}
	}
	loaded := target.view != nil && !target.view.IsDestroyed()
	switch {
	case request.Action == "close":
		if loaded {
			target.view.Destroy()
		}
		id := strconv.FormatUint(target.id, 10)
		delete(state.tabs, id)
		if !selected {
			r.App.DropPanelTab(panel.Tab{Kind: panel.Browser, ID: id})
		}
		r.App.Panel.Close(panel.Tab{Kind: panel.Browser, ID: id})
		return settled(browserValue(map[string]any{"closed": target.id}))
	case request.Action == "focus" && target.url == "":
		r.browserReveal(target)
		return settled(browserValue(map[string]any{"tab": target.id, "url": ""}))
	case request.Action != "navigate" && target.url == "":
		return settled(browserFailure(protocol.ErrorCodeNotFound, "browser tab is empty; navigate to a URL before reading or interacting"))
	case request.Action == "focus":
		r.browserReveal(target)
	}
	address := target.url
	if request.Action == "navigate" {
		if !request.Background {
			r.browserReveal(target)
		}
		target.background, address, loaded = request.Background, request.URL, false
	}
	if !loaded {
		if err := r.browserLoad(target, address); err != nil {
			return settled(browserUnavailable())
		}
	}
	if request.Action == "wait" && request.TimeoutMs == 0 {
		return settled(browserFailure(protocol.ErrorCodeInvalidRequest, "browser wait timeout must be positive"))
	}
	return browserPlan{tab: target, view: target.view}
}

// browserLoaded waits until the tab finishes loading or reports a failure.
func browserLoaded(ctx context.Context, plan browserPlan, deadline time.Time) *protocol.BrowserResult {
	for {
		type progress struct{ failed, loading bool }
		current, ran := browserMain(ctx, func() progress {
			if plan.view.IsDestroyed() {
				return progress{failed: true}
			}
			return progress{failed: plan.tab.failed(), loading: plan.view.Page().IsLoading()}
		})
		var failure protocol.BrowserResult
		switch {
		case !ran:
			failure = browserEnded(ctx)
		case current.failed:
			failure = browserFailure(protocol.ErrorCodeUnavailable, "browser page failed to load")
		case !current.loading:
			return nil
		case !time.Now().Before(deadline):
			failure = browserFailure(protocol.ErrorCodeUnavailable, "browser page is still loading; read it again")
		case !browserSleep(ctx, 50*time.Millisecond):
			failure = browserEnded(ctx)
		default:
			continue
		}
		return &failure
	}
}

// browserEvaluate runs the automation script with one typed request and
// decodes its answer. The page has five seconds to answer.
func browserEvaluate(ctx context.Context, view *mygo.WebView, request []byte) (map[string]any, *protocol.BrowserResult) {
	fail := func(result protocol.BrowserResult) (map[string]any, *protocol.BrowserResult) { return nil, &result }
	ctx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	type answer struct {
		value any
		err   error
	}
	done := make(chan answer, 1)
	go func() {
		value, err := view.Page().EvalContext(ctx, "return ("+browserAutomation+")("+string(request)+")")
		done <- answer{value, err}
	}()
	var raw string
	select {
	case result := <-done:
		text, ok := result.value.(string)
		if result.err != nil || !ok {
			return fail(browserUnavailable())
		}
		raw = text
	case <-ctx.Done():
		return fail(browserUnavailable())
	}
	var value map[string]any
	if json.Unmarshal([]byte(raw), &value) != nil || value == nil {
		return fail(browserUnavailable())
	}
	if message, failed := value["error"].(string); failed {
		return fail(browserFailure(protocol.ErrorCodeConflict, message))
	}
	return value, nil
}
