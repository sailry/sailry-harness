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

	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	maxSessions      = 4
	operationTimeout = 45 * time.Second
	// waitLimit is the longest one call waits for a page condition. It stays
	// under operationTimeout, so a condition that never holds is an answer
	// and not an interrupted call.
	waitLimit = 30 * time.Second
	// idleTimeout closes a browser that no call has used for this long.
	idleTimeout = 10 * time.Minute
)

type Options struct {
	// Profile is the Node profile directory. Browser profiles and temporary
	// files stay below it.
	Profile string
	Node    protocol.NodeID
	// Changed reports a worktree root that received a generated file.
	Changed func(root string)
	// Clock times idle browsers and plain waits. Nil is the real clock.
	Clock clockwork.Clock
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
	clock   clockwork.Clock
	// ctx ends when the Node stops: calls are interrupted and none starts.
	ctx  context.Context
	stop context.CancelFunc
	// processes parents every browser. It outlives ctx so that a stopping
	// Node can still ask its idle browsers to exit and flush their profiles.
	processes context.Context
	kill      context.CancelFunc

	mu       sync.Mutex
	sessions map[protocol.SessionID]*slot

	find   func() string // the installed browser; tests replace it
	launch func(ctx context.Context, id protocol.SessionID, executable string) (*session, error)
}

// slot serialises the operations of one conversation session. It exists
// while a call uses it or its browser runs. Everything but lock is guarded by
// Browsers.mu.
type slot struct {
	lock    chan struct{}
	session *session
	// starting reserves one of the Node's browsers while this one launches.
	starting bool
	// users counts the calls that hold or wait for lock. At zero the lock is
	// free, which is what lets an idle browser be closed without waiting.
	users int
	used  time.Time       // when the last call ended
	idle  clockwork.Timer // closes the browser after idleTimeout
}

func New(options Options) *Browsers {
	if options.Clock == nil {
		options.Clock = clockwork.NewRealClock()
	}
	processes, kill := context.WithCancel(context.Background())
	ctx, stop := context.WithCancel(processes)
	browsers := &Browsers{options: options, clock: options.Clock, ctx: ctx, stop: stop, processes: processes, kill: kill,
		sessions: map[protocol.SessionID]*slot{}, find: Installed}
	browsers.launch = browsers.start
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
	slot, started, err := browsers.acquire(ctx, call.Session, executable)
	if err != nil {
		return protocol.Output{}, err
	}
	defer browsers.release(call.Session, slot, true)
	if err := check(); err != nil {
		return protocol.Output{}, err
	}
	// The operation context ends with the turn, the Node or the deadline. The
	// browser itself belongs to the Node, so only this call is interrupted.
	operation, cancel := context.WithTimeout(slot.session.ctx, operationTimeout)
	defer cancel()
	defer context.AfterFunc(ctx, cancel)()
	defer context.AfterFunc(browsers.ctx, cancel)()
	return browsers.operate(operation, slot, call, arguments, started)
}

// operate holds the acquired slot until its worker has stopped, including when
// completion and cancellation race. Execute owns the slot's eventual release.
func (browsers *Browsers) operate(operation context.Context, slot *slot, call Call, arguments map[string]any, started bool) (protocol.Output, error) {
	type outcome struct {
		value map[string]any
		err   error
	}
	var result outcome
	// Completion can be observed by both the result and cancellation paths.
	// A cancellation racing a received result must not wait for a second send.
	done := make(chan struct{})
	go func() {
		value, err := browsers.run(operation, slot.session, call, arguments)
		result = outcome{value, err}
		close(done)
	}()
	select {
	case <-done:
		// A fault raised by an interrupted protocol call is not an answer.
		if operation.Err() == nil {
			if result.err != nil {
				return protocol.Output{}, result.err
			}
			if started && slot.session.resumed {
				// The conversation had a browser before, and the model may
				// still expect its pages.
				result.value["notice"] = "a new browser was started for this call; pages opened earlier are closed and cookies are kept"
			}
			return browsers.output(result.value), nil
		}
	case <-operation.Done():
	}
	// Only a call that did not come back in time, a stopped turn or a
	// stopping Node gets here. The browser's state is unknown, so it goes.
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
		staged, err := session.stageUpload(ctx, call.Root, arguments)
		if err != nil {
			return nil, err
		}
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

// retain finds or makes the session's slot for one more call. A browser in
// use is not idle, so its timer stops.
func (browsers *Browsers) retain(id protocol.SessionID) *slot {
	browsers.mu.Lock()
	defer browsers.mu.Unlock()
	current := browsers.sessions[id]
	if current == nil {
		current = &slot{lock: make(chan struct{}, 1)}
		current.lock <- struct{}{}
		browsers.sessions[id] = current
	}
	current.users++
	if current.idle != nil {
		current.idle.Stop()
		current.idle = nil
	}
	return current
}

// release ends one call's use of a slot. held says the call owns the lock.
// The last call leaves either an idle browser, which starts its timer, or
// nothing, and then the slot goes too.
func (browsers *Browsers) release(id protocol.SessionID, current *slot, held bool) {
	if held {
		current.lock <- struct{}{}
	}
	browsers.mu.Lock()
	defer browsers.mu.Unlock()
	current.users--
	if current.users > 0 {
		return
	}
	if current.session == nil {
		if browsers.sessions[id] == current {
			delete(browsers.sessions, id)
		}
		return
	}
	current.used = browsers.clock.Now()
	current.idle = browsers.clock.AfterFunc(idleTimeout, func() { browsers.expire(id, current) })
}

// detach takes an idle slot's browser away from it. The caller holds
// Browsers.mu, closes the returned browser outside it and then releases the
// slot it now holds.
func (browsers *Browsers) detach(current *slot) *session {
	<-current.lock // free, because no call uses the slot
	current.users++
	if current.idle != nil {
		current.idle.Stop()
		current.idle = nil
	}
	running := current.session
	current.session = nil
	return running
}

// expire closes a browser that stayed unused for idleTimeout. Its profile
// stays, so the conversation keeps its cookies.
func (browsers *Browsers) expire(id protocol.SessionID, current *slot) {
	browsers.mu.Lock()
	if browsers.sessions[id] != current || current.users != 0 || current.session == nil || browsers.clock.Since(current.used) < idleTimeout {
		browsers.mu.Unlock()
		return
	}
	running := browsers.detach(current)
	browsers.mu.Unlock()
	running.close(true)
	browsers.release(id, current, true)
}

// acquire locks the session's slot and starts its browser when none runs. It
// reports whether this call started the browser.
func (browsers *Browsers) acquire(ctx context.Context, id protocol.SessionID, executable string) (*slot, bool, error) {
	current := browsers.retain(id)
	select {
	case <-current.lock:
	case <-ctx.Done():
		browsers.release(id, current, false)
		return nil, false, fault(protocol.ErrorCodeCancelled, "external browser call cancelled")
	case <-browsers.ctx.Done():
		browsers.release(id, current, false)
		return nil, false, fault(protocol.ErrorCodeUnavailable, "execution Node is shutting down")
	}
	fail := func(err error) (*slot, bool, error) {
		browsers.release(id, current, true)
		return nil, false, err
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
	if current.session != nil {
		browsers.mu.Unlock()
		return current, false, nil
	}
	if browsers.ctx.Err() != nil {
		browsers.mu.Unlock()
		return fail(fault(protocol.ErrorCodeUnavailable, "execution Node is shutting down"))
	}
	// A new browser takes one of the Node's places while this lock still
	// holds the count, so two sessions never take the last one together.
	// When none is free, the browser that has been idle longest gives up its
	// place: its conversation keeps its profile and starts again on demand.
	running := 0
	var idlest *slot
	var evicted protocol.SessionID
	for other, candidate := range browsers.sessions {
		if candidate.session == nil && !candidate.starting {
			continue
		}
		running++
		if candidate.session != nil && candidate.users == 0 && (idlest == nil || candidate.used.Before(idlest.used)) {
			idlest, evicted = candidate, other
		}
	}
	var closing *session
	if running >= maxSessions {
		if idlest == nil {
			browsers.mu.Unlock()
			return fail(fault(protocol.ErrorCodeBusy, "every external browser session of this Node is in use; retry when one of them finishes its call"))
		}
		closing = browsers.detach(idlest)
	}
	current.starting = true
	browsers.mu.Unlock()
	if closing != nil {
		closing.close(true)
		browsers.release(evicted, idlest, true)
	}
	started, err := browsers.launch(ctx, id, executable)
	browsers.mu.Lock()
	current.starting = false
	if err == nil {
		current.session = started
	}
	browsers.mu.Unlock()
	if err != nil {
		return fail(err)
	}
	return current, true, nil
}

// Close ends the session's browser and waits for its process tree. The
// session's profile stays for its next browser.
func (browsers *Browsers) Close(id protocol.SessionID) { browsers.end(id, false) }

// Remove ends the browser of a deleted conversation and deletes its profile,
// which holds the cookies of the sites it signed in to.
func (browsers *Browsers) Remove(id protocol.SessionID) { browsers.end(id, true) }

func (browsers *Browsers) end(id protocol.SessionID, forget bool) {
	// The slot is held while the profile goes, so no browser starts on it.
	current := browsers.retain(id)
	<-current.lock
	browsers.mu.Lock()
	running := current.session
	current.session = nil
	browsers.mu.Unlock()
	if running != nil {
		running.close(true)
	}
	if forget && browsers.options.Profile != "" {
		_ = os.RemoveAll(filepath.Join(browsers.sessionsRoot(), id.String()))
	}
	browsers.release(id, current, true)
}

// Profiles lists the conversation sessions that have a browser profile on
// disk, whether or not a browser runs.
func (browsers *Browsers) Profiles() []protocol.SessionID {
	if browsers.options.Profile == "" {
		return nil
	}
	entries, _ := os.ReadDir(browsers.sessionsRoot())
	var ids []protocol.SessionID
	for _, entry := range entries {
		if id, err := protocol.ParseSessionID(entry.Name()); err == nil && entry.IsDir() {
			ids = append(ids, id)
		}
	}
	return ids
}

// Shutdown interrupts every call and closes every browser. It is final. A
// browser that was idle is asked to exit, so its profile is flushed.
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
	browsers.kill()
}

func (browsers *Browsers) sessionsRoot() string {
	return filepath.Join(browsers.options.Profile, "browser", "sessions")
}

// recover removes what a Node that did not stop cleanly left behind: browser
// processes that still use a session profile, and session temporary files.
// No process ID is recorded, so none can be mistaken after reuse: a process
// is reaped only when its own command line names a profile below this Node's
// private sessions directory, which nothing but a browser started here has.
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
