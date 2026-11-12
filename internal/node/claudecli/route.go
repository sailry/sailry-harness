package claudecli

import (
	"bufio"
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
)

// Child is a started Claude Code: its standard input and output as byte
// streams. It is the seam to the Node's process-owning layer, which owns the
// executable, consent, the process group and the scrubbed environment.
type Child interface {
	io.Writer
	io.Reader
	// Diagnostics returns the end of the child's error output.
	Diagnostics() string
	// Close ends the process and its descendants; a blocked Read returns.
	Close() error
}

// Launch is what the route asks the process layer to start.
type Launch struct {
	Arguments []string
	Set       map[string]string
	Unset     []string
	Directory string
}

// ErrNotInstalled is the launcher's answer when the user has no Claude Code.
var ErrNotInstalled = errors.New("Claude Code is not installed")

// Launcher starts the user's own claude with these arguments.
type Launcher func(Launch) (Child, error)

const (
	defaultIdle  = 10 * time.Minute
	defaultLimit = 4
	// listWait is how long changed tools wait to be listed before results go on.
	listWait = 5 * time.Second
)

type Options struct {
	Launcher Launcher
	// Executable is the Sailry binary Claude Code starts as the tool relay.
	Executable string
	// Directory is a private folder of the Node for run files.
	Directory string
	// Idle is how long an unused child is kept; Limit is how many are kept.
	Idle  time.Duration
	Limit int
}

// Route owns the children that serve conversations.
type Route struct {
	options Options
	mu      sync.Mutex
	runs    map[string]*run
	closed  bool
}

func NewRoute(options Options) *Route {
	if options.Idle <= 0 {
		options.Idle = defaultIdle
	}
	if options.Limit <= 0 {
		options.Limit = defaultLimit
	}
	return &Route{options: options, runs: make(map[string]*run)}
}

// run is one child and what it has taken of its conversation.
type run struct {
	child     Child
	bridge    *Bridge
	directory string
	events    chan []event
	done      chan struct{}
	once      sync.Once

	// The fields below are guarded by the Route's mutex.
	busy     bool
	settings string
	taken    int
	prefix   string
	pending  []string
	used     time.Time
	timer    *time.Timer
}

func (run *run) close() {
	run.once.Do(func() {
		close(run.done)
		if run.bridge != nil {
			run.bridge.Close()
		}
		_ = run.child.Close()
		_ = os.RemoveAll(run.directory)
	})
}

func (run *run) read() {
	defer close(run.events)
	decoder := newDecoder()
	reader := bufio.NewReaderSize(run.child, 64<<10)
	for {
		line, err := reader.ReadBytes('\n')
		if events := decoder.line(line); len(events) > 0 {
			select {
			case run.events <- events:
			case <-run.done:
				return
			}
		}
		if err != nil {
			return
		}
	}
}

// Close ends every child.
func (route *Route) Close() {
	route.mu.Lock()
	route.closed = true
	runs := route.runs
	route.runs = make(map[string]*run)
	route.mu.Unlock()
	for _, run := range runs {
		run.close()
	}
}

// Model returns the per-step model of one captured binding.
func (route *Route) Model(binding model.Binding) fantasy.LanguageModel {
	return &routeModel{route: route, binding: binding}
}

type routeModel struct {
	route   *Route
	binding model.Binding
}

func (*routeModel) Provider() string    { return "claude-code" }
func (value *routeModel) Model() string { return value.binding.Config.Model }

func (value *routeModel) Stream(ctx context.Context, call fantasy.Call) (fantasy.StreamResponse, error) {
	request, err := prepare(value.binding, call)
	if err != nil {
		return nil, err
	}
	return func(yield func(fantasy.StreamPart) bool) {
		if err := value.route.step(ctx, request, yield); err != nil {
			yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: model.Classify(err)})
		}
	}, nil
}

func (value *routeModel) Generate(ctx context.Context, call fantasy.Call) (*fantasy.Response, error) {
	stream, err := value.Stream(ctx, call)
	if err != nil {
		return nil, err
	}
	response := &fantasy.Response{}
	text, reasoning := map[string]int{}, map[string]int{}
	for part := range stream {
		switch part.Type {
		case fantasy.StreamPartTypeError:
			return nil, part.Error
		case fantasy.StreamPartTypeTextDelta:
			index, exists := text[part.ID]
			if !exists {
				index, text[part.ID] = len(response.Content), len(response.Content)
				response.Content = append(response.Content, fantasy.TextContent{})
			}
			content := response.Content[index].(fantasy.TextContent)
			content.Text += part.Delta
			response.Content[index] = content
		case fantasy.StreamPartTypeReasoningDelta:
			index, exists := reasoning[part.ID]
			if !exists {
				index, reasoning[part.ID] = len(response.Content), len(response.Content)
				response.Content = append(response.Content, fantasy.ReasoningContent{})
			}
			content := response.Content[index].(fantasy.ReasoningContent)
			content.Text += part.Delta
			response.Content[index] = content
		case fantasy.StreamPartTypeToolCall:
			response.Content = append(response.Content, fantasy.ToolCallContent{ToolCallID: part.ID, ToolName: part.ToolCallName, Input: part.ToolCallInput})
		case fantasy.StreamPartTypeFinish:
			response.Usage, response.FinishReason, response.ProviderMetadata = part.Usage, part.FinishReason, part.ProviderMetadata
		}
	}
	return response, nil
}

func (*routeModel) GenerateObject(context.Context, fantasy.ObjectCall) (*fantasy.ObjectResponse, error) {
	return nil, &model.Failure{Kind: model.Unsupported, Message: "structured generation is not configured"}
}

func (*routeModel) StreamObject(context.Context, fantasy.ObjectCall) (fantasy.ObjectStreamResponse, error) {
	return nil, &model.Failure{Kind: model.Unsupported, Message: "structured generation is not configured"}
}

// claim returns the conversation's child when the request extends what it
// has taken. A child the request diverged from is ended.
func (route *Route) claim(request stepRequest) (*run, error) {
	route.mu.Lock()
	defer route.mu.Unlock()
	if route.closed {
		return nil, &model.Failure{Kind: model.Unavailable, Message: "Claude Code route is closed"}
	}
	current := route.runs[request.key]
	if request.key == "" || current == nil || current.busy {
		return nil, nil
	}
	current.timer.Stop()
	if current.settings == request.settings && continues(current.taken, current.prefix, current.pending, request.messages) {
		current.busy = true
		return current, nil
	}
	delete(route.runs, request.key)
	go current.close()
	return nil, nil
}

// start launches a child for the request and tells it the whole conversation.
func (route *Route) start(request stepRequest) (*run, error) {
	directory, err := os.MkdirTemp(route.options.Directory, "run")
	if err != nil {
		return nil, err
	}
	work := filepath.Join(route.options.Directory, "work")
	if err := os.MkdirAll(work, 0o700); err != nil {
		_ = os.RemoveAll(directory)
		return nil, err
	}
	settings := Settings{Model: request.model, Effort: request.effort}
	started := &run{directory: directory, events: make(chan []event, 64), done: make(chan struct{}), settings: request.settings, busy: true}
	fail := func(err error) (*run, error) {
		if started.bridge != nil {
			started.bridge.Close()
		}
		_ = os.RemoveAll(directory)
		return nil, err
	}
	if len(request.tools) > 0 {
		if started.bridge, err = NewBridge(route.options.Directory); err != nil {
			return fail(err)
		}
		if _, err := started.bridge.SetTools(request.tools); err != nil {
			return fail(&model.Failure{Kind: model.InvalidRequest, Message: err.Error()})
		}
		settings.Config = started.bridge.Config(route.options.Executable)
	}
	if request.system != "" {
		settings.System = filepath.Join(directory, "system.md")
		if err := os.WriteFile(settings.System, []byte(request.system), 0o600); err != nil {
			return fail(err)
		}
	}
	set, unset := Environment()
	started.child, err = route.options.Launcher(Launch{Arguments: Arguments(settings), Set: set, Unset: unset, Directory: work})
	if errors.Is(err, ErrNotInstalled) {
		return fail(&model.Failure{Kind: model.NotConfigured, Message: ErrNotInstalled.Error()})
	}
	if err != nil {
		return fail(&model.Failure{Kind: model.Unavailable, Message: "Claude Code could not be started: " + err.Error()})
	}
	go started.read()
	line, err := userLine(replay(request.messages))
	if err == nil {
		_, err = started.child.Write(line)
	}
	if err != nil {
		started.close()
		return nil, route.ended(started)
	}
	return started, nil
}

// resume hands a living child the results and messages it has not seen.
func (route *Route) resume(ctx context.Context, current *run, request stepRequest, taken int, pending []string) error {
	rest := request.messages[taken+1:]
	results := map[string]Result{}
	users := []turnMessage{}
	for _, message := range rest {
		if message.role == roleTool {
			for identity, result := range message.results {
				results[identity] = result
			}
		} else {
			users = append(users, message)
		}
	}
	if current.bridge != nil {
		changed, err := current.bridge.SetTools(request.tools)
		if err != nil {
			return &model.Failure{Kind: model.InvalidRequest, Message: err.Error()}
		}
		if changed {
			// Claude Code takes up changed tools before the results: its next request offers them.
			current.bridge.Listed(ctx, listWait)
		}
	}
	if len(pending) == 0 {
		line, err := userLine(following(users))
		if err == nil {
			_, err = current.child.Write(line)
		}
		if err != nil {
			return route.ended(current)
		}
		return nil
	}
	for index, identity := range pending {
		result := results[identity]
		if index == len(pending)-1 {
			result = guidance(result, users)
		}
		current.bridge.Resolve(identity, result)
	}
	return nil
}

// ended is the failure of a child that stopped before its reply was complete.
func (route *Route) ended(current *run) error {
	text := strings.TrimSpace(current.child.Diagnostics())
	classified := failure(text, 0)
	if classified.Kind != model.Other {
		return classified
	}
	message := "Claude Code ended before the reply was complete"
	if text != "" {
		message += ": " + text
	}
	return &model.Failure{Kind: model.Unavailable, Message: message}
}

// release keeps a conversation's child for its next step or ends it.
func (route *Route) release(current *run, request stepRequest, keep bool, pending []string) {
	route.mu.Lock()
	keep = keep && request.key != "" && !route.closed
	if keep {
		if previous := route.runs[request.key]; previous != nil && previous != current {
			// Another step of this conversation owns the slot; this child was a one-off.
			keep = false
		}
	}
	if !keep {
		if route.runs[request.key] == current {
			delete(route.runs, request.key)
		}
		route.mu.Unlock()
		current.close()
		return
	}
	current.busy, current.taken, current.prefix, current.pending, current.used = false, len(request.messages), chain(request.messages), pending, time.Now()
	route.runs[request.key] = current
	current.timer = time.AfterFunc(route.options.Idle, func() { route.expire(current) })
	var evicted *run
	if len(route.runs) > route.options.Limit {
		for key, candidate := range route.runs {
			if candidate != current && !candidate.busy && (evicted == nil || candidate.used.Before(evicted.used)) {
				evicted = candidate
				_ = key
			}
		}
		if evicted != nil {
			for key, candidate := range route.runs {
				if candidate == evicted {
					delete(route.runs, key)
				}
			}
			evicted.timer.Stop()
		}
	}
	route.mu.Unlock()
	if evicted != nil {
		evicted.close()
	}
}

func (route *Route) expire(current *run) {
	route.mu.Lock()
	if current.busy {
		route.mu.Unlock()
		return
	}
	for key, candidate := range route.runs {
		if candidate == current {
			delete(route.runs, key)
		}
	}
	route.mu.Unlock()
	current.close()
}

// step runs one inference step: until Claude calls a Sailry tool or its turn ends.
func (route *Route) step(ctx context.Context, request stepRequest, yield func(fantasy.StreamPart) bool) error {
	current, err := route.claim(request)
	if err != nil {
		return err
	}
	if current != nil {
		// taken and pending are stable while the run is busy.
		if err := route.resume(ctx, current, request, current.taken, current.pending); err != nil {
			route.release(current, request, false, nil)
			return err
		}
	} else if current, err = route.start(request); err != nil {
		return err
	}
	pending, err := route.receive(ctx, current, request, yield)
	route.release(current, request, err == nil, pending)
	return err
}

// receive forwards the child's events until the step ends. It returns the
// identities of the calls the step ended on.
func (route *Route) receive(ctx context.Context, current *run, request stepRequest, yield func(fantasy.StreamPart) bool) ([]string, error) {
	var (
		counts   usage
		stop     string
		calls    []string
		ordinal  int
		said     bool
		rejected bool
		resets   time.Time
	)
	stopped := &model.Failure{Kind: model.Cancelled, Message: "model request was cancelled"}
	part := func(kind fantasy.StreamPartType, index int) fantasy.StreamPart {
		return fantasy.StreamPart{Type: kind, ID: fmt.Sprintf("claude-%d-%d", ordinal, index)}
	}
	finish := func(reason fantasy.FinishReason) bool {
		metadata := model.UsageMetadata{Reported: true, Precise: true, UncachedInput: counts.input, Output: counts.output, CacheRead: counts.cacheRead, CacheWrite: counts.cacheWrite}
		scalar := fantasy.Usage{}
		if counts.input != nil {
			input := *counts.input
			if counts.cacheRead != nil {
				input += *counts.cacheRead
				scalar.CacheReadTokens = *counts.cacheRead
			}
			if counts.cacheWrite != nil {
				input += *counts.cacheWrite
				scalar.CacheCreationTokens = *counts.cacheWrite
			}
			metadata.Input, scalar.InputTokens = &input, input
			if counts.output != nil {
				total := input + *counts.output
				metadata.Total, scalar.TotalTokens = &total, total
			}
		}
		if counts.output != nil {
			scalar.OutputTokens = *counts.output
		}
		return yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: reason, Usage: scalar,
			ProviderMetadata: fantasy.ProviderMetadata{model.UsageMetadataKey: &metadata}})
	}
	for {
		var batch []event
		var open bool
		select {
		case <-ctx.Done():
			return nil, stopped
		case batch, open = <-current.events:
		}
		if !open {
			if ctx.Err() != nil {
				return nil, stopped
			}
			return nil, route.ended(current)
		}
		for _, received := range batch {
			switch received.kind {
			case eventMessage:
				ordinal++
				calls = nil
			case eventUsage:
				counts.merge(received.usage)
				if received.stop != "" {
					stop = received.stop
				}
			case eventText, eventReasoning:
				if received.text == "" {
					continue
				}
				kind := fantasy.StreamPartTypeTextDelta
				if received.kind == eventReasoning {
					kind = fantasy.StreamPartTypeReasoningDelta
				}
				delta := part(kind, received.index)
				delta.Delta = received.text
				said = true
				if !yield(delta) {
					return nil, stopped
				}
			case eventReasoningEnd:
				if !yield(part(fantasy.StreamPartTypeReasoningEnd, received.index)) {
					return nil, stopped
				}
			case eventCall:
				declared, offered := request.names[strings.TrimPrefix(received.name, toolPrefix)]
				if !offered || !strings.HasPrefix(received.name, toolPrefix) {
					// A tool of Claude Code's own is Claude Code's to answer.
					continue
				}
				if received.identity == "" || !validArguments(received.arguments) {
					return nil, &model.Failure{Kind: model.InvalidResponse, Message: "Claude Code made a tool call without an identity or object arguments"}
				}
				calls = append(calls, received.identity)
				said = true
				if !yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall, ID: received.identity, ToolCallName: declared, ToolCallInput: string(received.arguments)}) {
					return nil, stopped
				}
			case eventMessageEnd:
				if len(calls) > 0 {
					if !finish(fantasy.FinishReasonToolCalls) {
						return nil, stopped
					}
					return calls, nil
				}
			case eventLimit:
				if received.rejected {
					rejected, resets = true, received.resets
				}
			case eventResult:
				if received.failed || (!said && marked(received.text)) {
					classified := failure(received.text, received.status)
					if rejected && (classified.Kind == model.Other || classified.Kind == model.RateLimited) {
						classified.Kind = model.RateLimited
						if wait := time.Until(resets); !resets.IsZero() && wait > 0 {
							classified.RetryAfter = &wait
						}
					}
					return nil, classified
				}
				reason := fantasy.FinishReasonStop
				if stop == "max_tokens" {
					reason = fantasy.FinishReasonLength
				}
				if !finish(reason) {
					return nil, stopped
				}
				return nil, nil
			}
		}
	}
}
