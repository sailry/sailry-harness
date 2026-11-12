package claudecli

import (
	"bufio"
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/program"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Capture is what the Node captured for one request to Claude Code.
type Capture struct {
	Adapter protocol.PluginModelAdapterDeclaration
	Program protocol.PluginProgramDeclaration
	// Owner is the package; Dir is the program's directory in its data and
	// the child's working folder. It is never a project.
	Owner, Dir string
	// Scratch is a private folder of the Node for run files; the Node empties
	// it when it starts.
	Scratch string
	// Host is the Node's environment as NAME=value entries.
	Host []string
	Pool *program.Pool
	// Executable is the Sailry binary Claude Code starts as the tool relay.
	Executable string
}

// errAbandoned ends a step whose consumer stopped reading.
var errAbandoned = errors.New("model stream was abandoned")

// listWait is how long changed tools wait to be listed before results go on.
const listWait = 5 * time.Second

// passed are variables of the user's own environment that configure their
// Claude Code and that the process layer does not inherit by itself.
var passed = []string{"CLAUDE_CONFIG_DIR", "DISABLE_TELEMETRY", "DO_NOT_TRACK", "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"}

// run is one child and what it has taken of its conversation. A step owns
// it while it holds the pool's lease.
type run struct {
	process   *program.Process
	bridge    *Bridge
	directory string
	events    chan []event
	done      chan struct{}
	once      sync.Once

	key      string
	settings string
	taken    int
	prefix   string
	pending  []string
}

// close removes what the run keeps beside its process.
func (run *run) close() {
	run.once.Do(func() {
		close(run.done)
		if run.bridge != nil {
			run.bridge.Close()
		}
		_ = os.RemoveAll(run.directory)
	})
}

func (run *run) read() {
	defer close(run.events)
	decoder := newDecoder()
	reader := bufio.NewReaderSize(run.process.Stdout, 64<<10)
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

// follows reports whether the request goes on in this run.
func (run *run) follows(request stepRequest) bool {
	return request.key != "" && run.key == request.key && run.settings == request.settings &&
		continues(run.taken, run.prefix, run.pending, request.messages)
}

// New returns the per-step model of one captured binding.
func New(binding model.Binding, capture Capture) (fantasy.LanguageModel, error) {
	if !model.PluginBound(binding.Provider) || capture.Adapter.ClaudeCode == nil || capture.Pool == nil ||
		(**binding.Provider.Binding).Adapter.Adapter != capture.Adapter.ID || capture.Adapter.ClaudeCode.Program != capture.Program.ID {
		return nil, &model.Failure{Kind: model.NotConfigured, Message: "captured model adapter is unavailable"}
	}
	return &routeModel{binding: binding, capture: capture}, nil
}

type routeModel struct {
	binding model.Binding
	capture Capture
}

func (*routeModel) Provider() string    { return "sailry-plugin" }
func (value *routeModel) Model() string { return value.binding.Config.Model }

func (value *routeModel) Stream(ctx context.Context, call fantasy.Call) (fantasy.StreamResponse, error) {
	request, err := prepare(value.binding, call)
	if err != nil {
		return nil, err
	}
	return func(yield func(fantasy.StreamPart) bool) {
		// A consumer that stopped reading is never called again.
		if err := value.step(ctx, request, yield); err != nil && !errors.Is(err, errAbandoned) {
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

// lease takes the conversation's child when the request extends what it has
// taken, else a new one told the whole conversation. A child the request
// diverged from is ended.
func (value *routeModel) lease(request stepRequest) (*program.Entry, *run, bool, error) {
	command := value.capture.Program.Command
	for {
		fresh := false
		entry, err := value.capture.Pool.Lease(value.capture.Owner, func(entry *program.Entry) int {
			current, ok := entry.Value.(*run)
			switch {
			case !ok || request.key == "" || current.key != request.key:
				return 0
			case current.follows(request):
				return 2
			}
			return 1
		}, func() (*program.Process, any, error) {
			fresh = true
			process, started, err := value.start(request)
			return process, started, err
		})
		if errors.Is(err, program.ErrBusy) {
			return nil, nil, false, &model.Failure{Kind: model.Unavailable, Message: fmt.Sprintf("every %s process is busy", command)}
		}
		if err != nil {
			return nil, nil, false, err
		}
		current := entry.Value.(*run)
		if fresh || current.follows(request) {
			return entry, current, fresh, nil
		}
		value.capture.Pool.Release(entry, false)
	}
}

// start launches a child for the request and tells it the whole conversation.
func (value *routeModel) start(request stepRequest) (*program.Process, *run, error) {
	declared := value.capture.Program
	if err := os.MkdirAll(value.capture.Scratch, 0o700); err != nil {
		return nil, nil, err
	}
	directory, err := os.MkdirTemp(value.capture.Scratch, "run")
	if err != nil {
		return nil, nil, err
	}
	settings := Settings{Model: request.model, Effort: request.effort}
	started := &run{directory: directory, events: make(chan []event, 64), done: make(chan struct{}), key: request.key, settings: request.settings}
	fail := func(err error) (*program.Process, *run, error) {
		started.close()
		return nil, nil, err
	}
	if len(request.tools) > 0 {
		if started.bridge, err = NewBridge(value.capture.Scratch); err != nil {
			return fail(err)
		}
		if _, err := started.bridge.SetTools(request.tools); err != nil {
			return fail(&model.Failure{Kind: model.InvalidRequest, Message: err.Error()})
		}
		settings.Config = started.bridge.Config(value.capture.Executable)
	}
	if request.system != "" {
		settings.System = filepath.Join(directory, "system.md")
		if err := os.WriteFile(settings.System, []byte(request.system), 0o600); err != nil {
			return fail(err)
		}
	}
	spec := program.Spec{Command: declared.Command, Args: Arguments(settings), Dir: value.capture.Dir, Host: value.capture.Host,
		Env: map[string]string{}, Log: value.capture.Owner + "/" + declared.ID}
	for _, entry := range value.capture.Host {
		if name, text, ok := strings.Cut(entry, "="); ok && slices.Contains(passed, name) {
			spec.Env[name] = text
		}
	}
	for name, text := range declared.Env {
		// This route holds no key, so a variable that asks for one is not set.
		if !strings.Contains(text, "{key}") {
			spec.Env[name] = strings.ReplaceAll(text, "{dir}", value.capture.Dir)
		}
	}
	for name, text := range Environment() {
		spec.Env[name] = text
	}
	process, err := program.Start(spec)
	if errors.Is(err, program.ErrNotInstalled) {
		return fail(&model.Failure{Kind: model.NotConfigured, Message: fmt.Sprintf("%s is not installed or not on the search path", declared.Command)})
	}
	if err != nil {
		return fail(&model.Failure{Kind: model.Unavailable, Message: fmt.Sprintf("%s could not be started", declared.Command)})
	}
	started.process = process
	// The pool ends the process; what the run keeps beside it goes then.
	go func() {
		<-process.Done()
		started.close()
	}()
	go started.read()
	line, err := userLine(replay(request.messages))
	if err == nil {
		_, err = process.Stdin.Write(line)
	}
	if err != nil {
		return process, nil, value.ended(started)
	}
	return process, started, nil
}

// resume hands a living child the results and messages it has not seen.
func (value *routeModel) resume(ctx context.Context, current *run, request stepRequest) error {
	rest := request.messages[current.taken+1:]
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
	if len(current.pending) == 0 {
		line, err := userLine(following(users))
		if err == nil {
			_, err = current.process.Stdin.Write(line)
		}
		if err != nil {
			return value.ended(current)
		}
		return nil
	}
	for index, identity := range current.pending {
		result := results[identity]
		if index == len(current.pending)-1 {
			result = guidance(result, users)
		}
		current.bridge.Resolve(identity, result)
	}
	return nil
}

// ended is the failure of a child that stopped before its reply was
// complete. The child is ended first, so its error output is whole.
func (value *routeModel) ended(current *run) error {
	current.process.Kill()
	text := strings.TrimSpace(current.process.Diagnostic())
	classified := value.failure(text, 0)
	if classified.Kind != model.Other {
		return classified
	}
	message := "Claude Code ended before the reply was complete"
	if text != "" {
		message += ": " + text
	}
	return &model.Failure{Kind: model.Unavailable, Message: message}
}

// failure classifies Claude Code's own failure text; a signed-out child is
// reported with the package's sign-in instruction.
func (value *routeModel) failure(text string, status int) *model.Failure {
	classified := failure(text, status)
	if classified.Kind == model.Authentication {
		classified.Message = "Claude Code is not signed in: " + value.capture.Adapter.ClaudeCode.SignIn
	}
	return classified
}

// step runs one inference step: until Claude calls a Sailry tool or its turn ends.
func (value *routeModel) step(ctx context.Context, request stepRequest, yield func(fantasy.StreamPart) bool) error {
	entry, current, fresh, err := value.lease(request)
	if err != nil {
		return err
	}
	keep := false
	// A child that is not kept is ended: a stopped or failed step, and a
	// request without tools.
	defer func() { value.capture.Pool.Release(entry, keep) }()
	if !fresh {
		if err := value.resume(ctx, current, request); err != nil {
			return err
		}
	}
	pending, err := value.receive(ctx, current, request, yield)
	if err != nil {
		return err
	}
	current.taken, current.prefix, current.pending = len(request.messages), chain(request.messages), pending
	keep = request.key != ""
	return nil
}

// receive forwards the child's events until the step ends. It returns the
// identities of the calls the step ended on.
func (value *routeModel) receive(ctx context.Context, current *run, request stepRequest, yield func(fantasy.StreamPart) bool) ([]string, error) {
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
			return nil, value.ended(current)
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
					return nil, errAbandoned
				}
			case eventReasoningEnd:
				if !yield(part(fantasy.StreamPartTypeReasoningEnd, received.index)) {
					return nil, errAbandoned
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
					return nil, errAbandoned
				}
			case eventMessageEnd:
				if len(calls) > 0 {
					if !finish(fantasy.FinishReasonToolCalls) {
						return nil, errAbandoned
					}
					return calls, nil
				}
			case eventLimit:
				if received.rejected {
					rejected, resets = true, received.resets
				}
			case eventResult:
				if received.failed || (!said && marked(received.text)) {
					classified := value.failure(received.text, received.status)
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
					return nil, errAbandoned
				}
				return nil, nil
			}
		}
	}
}
