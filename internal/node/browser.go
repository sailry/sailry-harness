package node

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"image"
	_ "image/jpeg"
	_ "image/png"
	"net/url"
	"sync"
	"time"

	"github.com/MoeForge/moego/base64"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Browser result bounds of the v1 browser contract.
const (
	browserMaxImageBytes  = 1024 * 1024
	browserMaxImageSide   = 1568
	browserMaxResultBytes = 256 * 1024
	browserCallTimeout    = 30 * time.Second
)

// browserRegistry routes browser tools to the initiating controller, not to
// another desktop. Its zero value is ready.
type browserRegistry struct {
	mu          sync.Mutex
	connections map[protocol.NodeID]*browserConnection
}

// browserConnection is one controller's subscription. replies is guarded by
// the registry mutex; closed ends the pending calls of a dropped subscription.
type browserConnection struct {
	id      protocol.RequestID
	calls   chan protocol.BrowserCall
	closed  chan struct{}
	replies map[protocol.RequestID]chan protocol.BrowserResult
}

type browserCalls struct {
	owner      *browserRegistry
	caller     protocol.NodeID
	connection *browserConnection
	close      sync.Once
}

func browserFault(code protocol.ErrorCode, message string) *protocol.Fault {
	return &protocol.Fault{Code: code, Message: message}
}

func browserDisconnected() *protocol.Fault {
	return browserFault(protocol.ErrorCodeUnavailable, "browser controller is disconnected")
}

func (registry *browserRegistry) available(caller protocol.NodeID) bool {
	registry.mu.Lock()
	defer registry.mu.Unlock()
	return registry.connections[caller] != nil
}

func (registry *browserRegistry) subscribe(caller protocol.NodeID) (link.Subscription, error) {
	registry.mu.Lock()
	defer registry.mu.Unlock()
	if registry.connections[caller] != nil {
		return nil, browserFault(protocol.ErrorCodeConflict, "browser controller is already connected")
	}
	if registry.connections == nil {
		registry.connections = map[protocol.NodeID]*browserConnection{}
	}
	connection := &browserConnection{id: protocol.NewRequestID(), calls: make(chan protocol.BrowserCall, 16), closed: make(chan struct{}),
		replies: map[protocol.RequestID]chan protocol.BrowserResult{}}
	registry.connections[caller] = connection
	return &browserCalls{owner: registry, caller: caller, connection: connection}, nil
}

func (calls *browserCalls) Next(ctx context.Context) (protocol.Update, error) {
	select {
	case call := <-calls.connection.calls:
		return protocol.Update{Kind: "browser_call", Data: call}, nil
	case <-calls.connection.closed:
		return protocol.Update{}, browserDisconnected()
	case <-ctx.Done():
		return protocol.Update{}, ctx.Err()
	}
}

func (calls *browserCalls) Close() error {
	calls.close.Do(func() {
		calls.owner.mu.Lock()
		defer calls.owner.mu.Unlock()
		if current := calls.owner.connections[calls.caller]; current != nil && current.id == calls.connection.id {
			delete(calls.owner.connections, calls.caller)
		}
		calls.connection.replies = map[protocol.RequestID]chan protocol.BrowserResult{}
		close(calls.connection.closed)
	})
	return nil
}

// call sends one action to the caller's browser and waits for its completion.
// A lost reply is not permission to replay an action with side effects.
func (registry *browserRegistry) call(ctx context.Context, caller protocol.NodeID, session protocol.SessionID, id protocol.RequestID, action protocol.BrowserAction) (json.RawMessage, error) {
	readOnly := browserReadOnly(action)
	registry.mu.Lock()
	connection := registry.connections[caller]
	if connection == nil {
		registry.mu.Unlock()
		return nil, browserDisconnected()
	}
	if _, pending := connection.replies[id]; pending {
		registry.mu.Unlock()
		return nil, browserFault(protocol.ErrorCodeConflict, "browser call is already pending")
	}
	reply := make(chan protocol.BrowserResult, 1)
	connection.replies[id] = reply
	sent := false
	select {
	case connection.calls <- protocol.BrowserCall{ID: id, Session: session, Action: action, ExpiresAtMs: uint64(time.Now().UnixMilli()) + uint64(browserCallTimeout/time.Millisecond)}:
		sent = true
	default:
		delete(connection.replies, id)
	}
	registry.mu.Unlock()
	if !sent {
		return nil, browserDisconnected()
	}
	defer func() {
		registry.mu.Lock()
		if connection.replies[id] == reply {
			delete(connection.replies, id)
		}
		registry.mu.Unlock()
	}()
	timer := time.NewTimer(browserCallTimeout)
	defer timer.Stop()
	select {
	case result := <-reply:
		if result.Err != nil {
			return nil, result.Err
		}
		return *result.Ok, nil
	case <-ctx.Done():
		code := protocol.ErrorCodeOutcomeUnknown
		if readOnly {
			code = protocol.ErrorCodeCancelled
		}
		return nil, browserFault(code, "browser call interrupted; read the page before trying another action")
	case <-connection.closed:
	case <-timer.C:
	}
	code := protocol.ErrorCodeOutcomeUnknown
	if readOnly {
		code = protocol.ErrorCodeUnavailable
	}
	return nil, browserFault(code, "browser response unavailable; read the page before trying another action")
}

func (registry *browserRegistry) complete(caller protocol.NodeID, id protocol.RequestID, result protocol.BrowserResult) error {
	if !browserBounded(result) {
		return browserFault(protocol.ErrorCodeInvalidRequest, "browser result is too large")
	}
	registry.mu.Lock()
	defer registry.mu.Unlock()
	connection := registry.connections[caller]
	if connection == nil {
		return browserDisconnected()
	}
	reply := connection.replies[id]
	if reply == nil {
		return browserFault(protocol.ErrorCodeNotFound, "browser call is no longer pending")
	}
	delete(connection.replies, id)
	reply <- result
	return nil
}

// browserBounded gives a capture's encoded image its own bound beside the
// ordinary result limit.
func browserBounded(result protocol.BrowserResult) bool {
	if (result.Ok == nil) == (result.Err == nil) {
		return false
	}
	size := 0
	if result.Ok != nil {
		var facts struct {
			Image *string `json:"base64_image"`
		}
		if json.Unmarshal(*result.Ok, &facts) == nil && facts.Image != nil {
			size = len(*facts.Image)
		}
	}
	encoded, err := json.Marshal(result)
	return err == nil && size <= (browserMaxImageBytes+2)/3*4 && len(encoded) <= browserMaxResultBytes+size
}

// browserFacts are the action fields the Node decides on.
type browserFacts struct {
	Action    string `json:"action"`
	URL       string `json:"url"`
	TimeoutMs uint32 `json:"timeout_ms"`
	Condition struct {
		Text     string `json:"text"`
		Selector string `json:"selector"`
	} `json:"condition"`
	Format protocol.BrowserFormat `json:"format"`
	Save   bool                   `json:"save"`
}

func browserActionFacts(action protocol.BrowserAction) (browserFacts, error) {
	var facts browserFacts
	encoded, err := json.Marshal(action)
	if err == nil {
		err = json.Unmarshal(encoded, &facts)
	}
	if err != nil {
		return facts, browserFault(protocol.ErrorCodeInvalidRequest, "invalid browser operation")
	}
	return facts, nil
}

func browserReadOnly(action protocol.BrowserAction) bool {
	switch action.Kind {
	case "tabs", "read", "wait", "screenshot":
		return true
	}
	return false
}

func browserRequiresApproval(action protocol.BrowserAction) bool {
	switch action.Kind {
	case "tabs", "read", "navigate", "scroll", "back", "forward", "refresh", "open", "focus", "frame", "wait":
		return false
	case "screenshot":
		facts, err := browserActionFacts(action)
		return err != nil || facts.Save
	}
	return true
}

func validateBrowserAction(action protocol.BrowserAction) error {
	facts, err := browserActionFacts(action)
	if err != nil {
		return err
	}
	if action.Kind == "navigate" || action.Kind == "open" {
		target, err := url.Parse(facts.URL)
		if err != nil || target.Scheme != "http" && target.Scheme != "https" || target.Host == "" || target.User != nil {
			return browserFault(protocol.ErrorCodeInvalidRequest, "browser URL must use HTTP or HTTPS without embedded credentials")
		}
	}
	if action.Kind == "wait" && (facts.TimeoutMs == 0 || facts.Condition.Text == "" && facts.Condition.Selector == "") {
		return browserFault(protocol.ErrorCodeInvalidRequest, "invalid browser wait condition or timeout")
	}
	return nil
}

// useBrowser runs one admitted browser command. Node admission owns authority
// and saved captures; the initiating controller owns its WebView.
func (node *Node) useBrowser(ctx context.Context, caller protocol.NodeID, id protocol.RequestID, input protocol.CommandUseBrowserPayload) (protocol.Output, error) {
	if err := validateBrowserAction(input.Action); err != nil {
		return protocol.Output{}, err
	}
	facts, err := browserActionFacts(input.Action)
	if err != nil {
		return protocol.Output{}, err
	}
	if ctx.Err() != nil {
		return protocol.Output{}, browserFault(protocol.ErrorCodeCancelled, "browser call cancelled")
	}
	result, err := node.browsers.call(ctx, caller, input.Session, id, input.Action)
	if err != nil {
		return protocol.Output{}, err
	}
	if input.Action.Kind == "screenshot" {
		pixels, err := browserCapture(result, facts.Format)
		if err != nil {
			return protocol.Output{}, err
		}
		if facts.Save {
			// A saved capture is published like generated media: below
			// assets/generated, never replacing a file.
			var root string
			if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
				var err error
				root, err = store.ReadWorkspaceRoot(ctx, db, input.Worktree, false)
				return err
			}); err != nil {
				return protocol.Output{}, nodeFault(err)
			}
			extension := "jpg"
			if facts.Format == protocol.BrowserFormatPng {
				extension = "png"
			}
			target := "assets/generated/browser-" + id.String() + "." + extension
			if err := node.publishGenerated(ctx, root, target, pixels.data, browserMaxImageBytes); err != nil {
				return protocol.Output{}, err
			}
			pixels.facts["path"], _ = json.Marshal(target)
		}
		if result, err = json.Marshal(pixels.facts); err != nil {
			return protocol.Output{}, err
		}
	}
	return protocol.Output{Kind: "browser", Data: result}, nil
}

// browserPixels is one checked capture: the facts published with it, its
// inline media among them, and its bytes.
type browserPixels struct {
	facts map[string]json.RawMessage
	data  []byte
}

// browserCapture checks the controller's pixels and declares them as inline
// media. The controller supplies pixels and page facts; the Node owns the
// encoding checks and paths.
func browserCapture(value json.RawMessage, format protocol.BrowserFormat) (browserPixels, error) {
	invalid := func(message string) (browserPixels, error) {
		return browserPixels{}, browserFault(protocol.ErrorCodeInvalidRequest, message)
	}
	var facts map[string]json.RawMessage
	if json.Unmarshal(value, &facts) != nil || facts == nil {
		return invalid("browser capture is invalid")
	}
	encoded, exists := facts["base64_image"]
	if !exists {
		return invalid("browser capture is missing image data")
	}
	for _, reserved := range []string{"base64_image", "path", "inline_data", "file_data", "images"} {
		delete(facts, reserved)
	}
	var text string
	_ = json.Unmarshal(encoded, &text)
	pixels, err := base64.StdEncoding.DecodeString(text)
	if err != nil {
		return invalid("browser capture contains invalid image data")
	}
	mime := "image/" + string(format)
	bounded := len(pixels) <= browserMaxImageBytes
	var config image.Config
	if bounded {
		var decoded string
		config, decoded, err = image.DecodeConfig(bytes.NewReader(pixels))
		bounded = err == nil && decoded == string(format) && config.Width > 0 && config.Height > 0 && max(config.Width, config.Height) <= browserMaxImageSide
	}
	if !bounded {
		return invalid("browser capture must be a bounded image of the requested format")
	}
	facts["mime_type"], _ = json.Marshal(mime)
	facts["width"], _ = json.Marshal(config.Width)
	facts["height"], _ = json.Marshal(config.Height)
	facts["inline_data"], _ = json.Marshal([]map[string]string{{"mime_type": mime, "data": base64.StdEncoding.EncodeToString(pixels)}})
	return browserPixels{facts: facts, data: pixels}, nil
}

// dispatchBrowser completes a controller's browser call inline and admits a
// controller's own browser command durably.
func (node *Node) dispatchBrowser(ctx context.Context, caller protocol.NodeID, request protocol.Request, body []byte) (link.Admission, error) {
	completion := make(chan link.Response, 1)
	result := link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: completion}
	if input, ok := request.Command.Data.(*protocol.CommandCompleteBrowserPayload); ok {
		if request.Plugin != nil {
			return link.Admission{}, browserFault(protocol.ErrorCodePermissionDenied, "browser completion requires a controller")
		}
		if err := node.begin(); err != nil {
			return link.Admission{}, err
		}
		defer node.work.Done()
		if err := node.browsers.complete(caller, input.ID, input.Result); err != nil {
			return link.Admission{}, err
		}
		completion <- link.Response{Output: protocol.Output{Kind: "browser_completed"}}
		close(completion)
		return result, nil
	}
	input, ok := request.Command.Data.(*protocol.CommandUseBrowserPayload)
	if !ok {
		return link.Admission{}, browserFault(protocol.ErrorCodeInvalidRequest, "invalid browser operation")
	}
	node.mu.Lock()
	defer node.mu.Unlock()
	if node.stopped {
		return link.Admission{}, unavailable()
	}
	if err := ctx.Err(); err != nil {
		return link.Admission{}, nodeFault(err)
	}
	var previous *link.Response
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		previous, err = admission.Admit(ctx, db, caller, request, body)
		return err
	}); err != nil {
		return link.Admission{}, nodeFault(err)
	}
	if previous != nil {
		completion <- *previous
		close(completion)
		return result, nil
	}
	node.work.Add(1)
	go func() {
		defer node.work.Done()
		defer close(completion)
		output, effectErr := node.useBrowser(node.ctx, caller, request.ID, *input)
		completion <- node.settle(caller, request.ID, response(output, effectErr))
	}()
	return result, nil
}
