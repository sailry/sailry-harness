// Package externalbrowser owns the managed Chromium sessions of one Node. Each
// conversation session drives one browser process tree with its own user data
// directory through chromedp. It never attaches to a browser the user runs.
package externalbrowser

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	maxSessions      = 4
	operationTimeout = 45 * time.Second
)

type Options struct {
	// Profile is the Node profile directory. Browser profiles and temporary
	// files stay below it.
	Profile string
	Node    protocol.NodeID
	// Changed reports a worktree root that received a generated file.
	Changed func(root string)
}

// Call is one admitted use_external_browser operation.
type Call struct {
	Request   protocol.RequestID
	Session   protocol.SessionID
	Root      string // worktree root that confines uploads and saved files
	Action    protocol.ExternalBrowserAction
	Arguments json.RawMessage
	// Check repeats the plugin authority decision around browser startup.
	Check func(context.Context) error
}

type Browsers struct {
	options Options
	ctx     context.Context
	stop    context.CancelFunc

	mu       sync.Mutex
	sessions map[protocol.SessionID]*slot

	find func() string // the installed browser; tests replace it
}

// slot serialises the operations of one conversation session.
type slot struct {
	lock    chan struct{}
	session *session
}

func New(options Options) *Browsers {
	ctx, stop := context.WithCancel(context.Background())
	browsers := &Browsers{options: options, ctx: ctx, stop: stop, sessions: map[protocol.SessionID]*slot{}, find: Installed}
	browsers.recover()
	return browsers
}

// Available reports whether this Node owns browser sessions at all. A missing
// browser is reported by each call instead, so that its fault can be read.
func (browsers *Browsers) Available() bool {
	return browsers.ctx.Err() == nil && browsers.options.Profile != ""
}

// Sessions lists the conversation sessions that own a running browser.
func (browsers *Browsers) Sessions() []protocol.SessionID {
	browsers.mu.Lock()
	defer browsers.mu.Unlock()
	var ids []protocol.SessionID
	for id, slot := range browsers.sessions {
		if slot.session != nil {
			ids = append(ids, id)
		}
	}
	return ids
}

func (browsers *Browsers) Execute(ctx context.Context, call Call) (protocol.Output, error) {
	executable, err := browsers.Browser()
	if err != nil {
		return protocol.Output{}, err
	}
	arguments, err := Validate(call.Action, call.Arguments)
	if err != nil {
		return protocol.Output{}, err
	}
	check := func() error {
		if call.Check == nil {
			return nil
		}
		return call.Check(ctx)
	}
	if err := check(); err != nil {
		return protocol.Output{}, err
	}
	if ctx.Err() != nil || browsers.ctx.Err() != nil {
		return protocol.Output{}, fault(protocol.ErrorCodeCancelled, "external browser call cancelled")
	}
	if call.Action == protocol.ExternalBrowserActionCloseSession {
		browsers.Close(call.Session)
		return browsers.output(map[string]any{"closed": true, "execution_node": browsers.options.Node}), nil
	}
	slot, err := browsers.acquire(ctx, call.Session, executable)
	if err != nil {
		return protocol.Output{}, err
	}
	defer func() { slot.lock <- struct{}{} }()
	if err := check(); err != nil {
		return protocol.Output{}, err
	}
	// The operation context ends with the turn, the Node or the deadline. The
	// browser itself belongs to the Node, so only this call is interrupted.
	operation, cancel := context.WithTimeout(slot.session.ctx, operationTimeout)
	defer cancel()
	release := context.AfterFunc(ctx, cancel)
	defer release()
	type outcome struct {
		value map[string]any
		err   error
	}
	done := make(chan outcome, 1)
	go func() {
		value, err := browsers.run(operation, slot.session, call, arguments)
		done <- outcome{value, err}
	}()
	select {
	case result := <-done:
		// A fault raised by an interrupted protocol call is not an answer.
		if operation.Err() == nil {
			if result.err != nil {
				return protocol.Output{}, result.err
			}
			return browsers.output(result.value), nil
		}
	case <-operation.Done():
	}
	slot.session.close(false)
	<-done
	browsers.mu.Lock()
	slot.session = nil
	browsers.mu.Unlock()
	if call.Action.ReadOnly() {
		return protocol.Output{}, fault(protocol.ErrorCodeCancelled, "external browser call interrupted; browser session closed")
	}
	return protocol.Output{}, fault(protocol.ErrorCodeOutcomeUnknown, "external browser call interrupted; effects may have occurred; browser session closed; verify before retrying")
}

func (browsers *Browsers) output(value map[string]any) protocol.Output {
	encoded, _ := json.Marshal(value)
	return protocol.Output{Kind: "external_browser", Data: json.RawMessage(encoded)}
}

// run executes one operation and shapes its bounded result.
func (browsers *Browsers) run(ctx context.Context, session *session, call Call, arguments map[string]any) (map[string]any, error) {
	node := browsers.options.Node
	switch call.Action {
	case protocol.ExternalBrowserActionDownloads:
		return session.downloads(node)
	case protocol.ExternalBrowserActionSaveDownload:
		return browsers.saveDownload(ctx, session, call, arguments)
	}
	original, uploading := arguments["file_path"]
	if call.Action == protocol.ExternalBrowserActionFileUpload {
		staged, cleanup, err := stageUpload(ctx, session, call.Root, arguments)
		if err != nil {
			return nil, err
		}
		defer cleanup()
		arguments["file_path"] = staged
	}
	result, err := session.execute(ctx, call.Action, arguments)
	if err != nil {
		return nil, err
	}
	if uploading {
		result["uploaded_file"] = original
	}
	media, err := browsers.artifact(ctx, call, result, arguments["save_to_artifacts"] == true)
	if err != nil {
		return nil, err
	}
	remaining := resultBytes
	bounded, truncated := bound(result, &remaining)
	output := map[string]any{"execution_node": node, "result": bounded, "truncated": truncated}
	if media != nil {
		output["inline_data"] = []any{media}
	}
	return output, nil
}

// acquire locks the session's slot and starts its browser when none runs.
func (browsers *Browsers) acquire(ctx context.Context, id protocol.SessionID, executable string) (*slot, error) {
	browsers.mu.Lock()
	current := browsers.sessions[id]
	if current == nil {
		current = &slot{lock: make(chan struct{}, 1)}
		current.lock <- struct{}{}
		browsers.sessions[id] = current
	}
	browsers.mu.Unlock()
	select {
	case <-current.lock:
	case <-ctx.Done():
		return nil, fault(protocol.ErrorCodeCancelled, "external browser call cancelled")
	case <-browsers.ctx.Done():
		return nil, fault(protocol.ErrorCodeUnavailable, "execution Node is shutting down")
	}
	fail := func(err error) (*slot, error) {
		current.lock <- struct{}{}
		return nil, err
	}
	browsers.mu.Lock()
	if current.session != nil && current.session.ctx.Err() != nil {
		// The browser exited on its own; its files are reclaimed here.
		exited := current.session
		current.session = nil
		browsers.mu.Unlock()
		exited.close(false)
		browsers.mu.Lock()
	}
	running := 0
	for _, other := range browsers.sessions {
		if other.session != nil {
			running++
		}
	}
	live := current.session != nil
	browsers.mu.Unlock()
	if live {
		return current, nil
	}
	if browsers.ctx.Err() != nil {
		return fail(fault(protocol.ErrorCodeUnavailable, "execution Node is shutting down"))
	}
	if running >= maxSessions {
		return fail(fault(protocol.ErrorCodeBusy, "close an external browser session before starting another"))
	}
	started, err := browsers.start(ctx, id, executable)
	if err != nil {
		return fail(err)
	}
	browsers.mu.Lock()
	current.session = started
	browsers.mu.Unlock()
	return current, nil
}

// Close ends the session's browser and waits for its process tree.
func (browsers *Browsers) Close(id protocol.SessionID) {
	browsers.mu.Lock()
	current := browsers.sessions[id]
	browsers.mu.Unlock()
	if current == nil {
		return
	}
	<-current.lock
	browsers.mu.Lock()
	running := current.session
	current.session = nil
	browsers.mu.Unlock()
	if running != nil {
		running.close(true)
	}
	current.lock <- struct{}{}
}

// Shutdown interrupts every call and closes every browser. It is final.
func (browsers *Browsers) Shutdown() {
	browsers.stop()
	browsers.mu.Lock()
	ids := make([]protocol.SessionID, 0, len(browsers.sessions))
	for id := range browsers.sessions {
		ids = append(ids, id)
	}
	browsers.mu.Unlock()
	var wait sync.WaitGroup
	for _, id := range ids {
		wait.Go(func() { browsers.Close(id) })
	}
	wait.Wait()
}

func (browsers *Browsers) sessionsRoot() string {
	return filepath.Join(browsers.options.Profile, "browser", "sessions")
}

// recover removes what a Node that did not stop cleanly left behind: browser
// processes that still use a session profile, and session temporary files.
func (browsers *Browsers) recover() {
	if browsers.options.Profile == "" {
		return
	}
	if _, err := os.Stat(browsers.sessionsRoot()); err == nil {
		killProcessesUsing(browsers.sessionsRoot() + string(filepath.Separator))
	}
	entries, _ := os.ReadDir(filepath.Join(browsers.options.Profile, "tmp"))
	for _, entry := range entries {
		if entry.IsDir() && strings.HasPrefix(entry.Name(), "browser-") {
			_ = os.RemoveAll(filepath.Join(browsers.options.Profile, "tmp", entry.Name()))
		}
	}
}

func fault(code protocol.ErrorCode, message string) error {
	return &protocol.Fault{Code: code, Message: message}
}

func unavailable(message string) error { return fault(protocol.ErrorCodeUnavailable, message) }
