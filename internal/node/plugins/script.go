package plugins

import (
	"context"
	"embed"
	"encoding/json"
	"errors"
	"path"
	"strings"
	"time"

	"github.com/MoeForge/moejs"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const ScriptMemoryBytes = 64 * 1024 * 1024

const ScriptCallTimeout = 60 * time.Second
const ScriptMaxDataBytes = 2 * 1024 * 1024
const ScriptMaxBytes = 256 * 1024

//go:embed script/*.js script/*.d.ts
var scriptFiles embed.FS

type ScriptBundle struct {
	Entry string
	Files map[string]string
}

type ScriptEnvironment struct {
	Context  protocol.PluginContext
	Target   protocol.NodeID
	Deadline time.Time
}

type ScriptDispatch func(context.Context, protocol.Request) (protocol.Output, error)

type ScriptExecution struct {
	CallID     *protocol.RequestID
	Mode       protocol.WorkMode
	ReadOnly   bool
	Turn       json.RawMessage
	StagedTurn json.RawMessage
	// Writes counts the durable writes of this tool call: in the first
	// evaluation's count, out the count the next evaluation of the same
	// call continues from, so each write has its own request ID.
	Writes uint64
}

type scriptVM struct {
	vm           *moejs.Runtime
	ctx          context.Context
	deadline     time.Time
	exports      moejs.Value
	invoke       moejs.Value
	inspect      moejs.Value
	has          moejs.Value
	releaseRoots func()
	bridge       *scriptBridge
	pure         bool
}

func RunCallback(ctx context.Context, bundle ScriptBundle, handler string, input any, environment ScriptEnvironment, readOnly bool, dispatch ScriptDispatch) (json.RawMessage, error) {
	var execution *ScriptExecution
	if readOnly {
		execution = &ScriptExecution{Mode: protocol.WorkModeCode, ReadOnly: true}
	}
	return runScript(ctx, bundle, handler, input, environment, execution, dispatch)
}

func RunTool(ctx context.Context, bundle ScriptBundle, handler string, input any, environment ScriptEnvironment, dispatch ScriptDispatch, execution *ScriptExecution) (json.RawMessage, error) {
	if execution == nil {
		return nil, scriptInvalid("tool execution state is unavailable")
	}
	return runScript(ctx, bundle, handler, input, environment, execution, dispatch)
}

func runScript(ctx context.Context, bundle ScriptBundle, handler string, input any, environment ScriptEnvironment, execution *ScriptExecution, dispatch ScriptDispatch) (json.RawMessage, error) {
	started := time.Now()
	deadline := environment.Deadline
	if deadline.IsZero() || deadline.After(started.Add(ScriptCallTimeout)) {
		deadline = started.Add(ScriptCallTimeout)
	}
	if contextual, ok := ctx.Deadline(); ok && contextual.Before(deadline) {
		deadline = contextual
	}
	bridge := &scriptBridge{
		ctx: ctx, deadline: deadline, target: environment.Target, scope: environment.Context,
		execution: execution, dispatch: dispatch,
		drafts: map[string]scriptDraft{}, faults: map[string]*protocol.Fault{},
		installations: map[protocol.RequestID]bool{},
	}
	runtime, err := newScriptVM(ctx, bundle, deadline, bridge, false)
	if err != nil {
		return nil, scriptDiagnostic(err, environment.Context, started, deadline)
	}
	defer runtime.close()
	result, err := runtime.call(handler, input, ScriptMaxDataBytes)
	if bridge.failure != nil {
		err = bridge.failure
	}
	if err != nil {
		return nil, scriptDiagnostic(err, environment.Context, started, deadline)
	}
	return result, nil
}

func newScriptVM(ctx context.Context, bundle ScriptBundle, deadline time.Time, bridge *scriptBridge, pure bool) (_ *scriptVM, result error) {
	if err := scriptCheck(ctx, deadline, pure); err != nil {
		return nil, err
	}
	runtime := &scriptVM{ctx: ctx, deadline: deadline, bridge: bridge, pure: pure}
	builtins := map[string]string{}
	names := []string{"sdk", "http", "models", "decisions", "connectors", "cards", "installations"}
	if pure {
		names = []string{"adapter"}
	}
	for _, name := range names {
		source, err := scriptFiles.ReadFile("script/" + name + ".js")
		if err != nil {
			return nil, scriptEngine(err)
		}
		builtins["sailry/"+name] = string(source)
	}
	if _, ok := bundle.Files[bundle.Entry]; !ok {
		return nil, scriptInvalid("plugin host entry is missing")
	}
	if pure {
		for name := range bundle.Files {
			if !scriptTranslationResource(name) {
				return nil, scriptInvalid("plugin translation resource is outside its namespace")
			}
		}
	}
	modules := map[string]*moejs.Module{}
	resolve := func(referrer moejs.Referrer, name string) (*moejs.Module, error) {
		if err := runtime.arm(); err != nil {
			return nil, err
		}
		if strings.HasPrefix(name, ".") && referrer != nil {
			name = path.Join(path.Dir(referrer.Name()), name)
		}
		if compiled := modules[name]; compiled != nil {
			return compiled, nil
		}
		source, exists := builtins[name]
		if !exists {
			source, exists = bundle.Files[name]
		}
		if !exists {
			return nil, scriptInvalid("plugin module is unavailable")
		}
		bounded, cancel := context.WithDeadline(ctx, runtime.deadline)
		defer cancel()
		compiled, err := moejs.CompileWith(name, source, moejs.CompileOptions{Context: bounded})
		if err != nil {
			return nil, err
		}
		modules[name] = compiled
		return compiled, nil
	}
	vm := moejs.NewRuntime(moejs.Options{
		MutableIntrinsics: true, MemoryLimit: ScriptMemoryBytes, Importer: &moejs.Importer{Resolve: resolve},
	})
	runtime.vm = vm
	runtime.releaseRoots = vm.AddLiveRoots(func(add func(moejs.Value)) {
		add(runtime.exports)
		add(runtime.invoke)
		add(runtime.inspect)
		add(runtime.has)
	})
	defer func() {
		if result != nil {
			runtime.close()
		}
	}()
	stop, err := runtime.begin()
	if err != nil {
		return nil, err
	}
	defer stop()
	bounded, cancel := context.WithDeadline(ctx, deadline)
	defer cancel()
	prelude, err := moejs.CompileScriptWith("sailry:intrinsics", scriptIntrinsics, moejs.CompileOptions{Context: bounded})
	if err != nil {
		return nil, runtime.engine(err)
	}
	intrinsics, err := vm.RunScript(prelude)
	if err != nil {
		return nil, runtime.engine(err)
	}
	for name, target := range map[string]*moejs.Value{
		"invoke": &runtime.invoke, "inspect": &runtime.inspect, "has": &runtime.has,
	} {
		value, err := vm.Get(intrinsics, name)
		if err != nil {
			return nil, runtime.engine(err)
		}
		*target = value
	}
	if bridge != nil {
		if err := vm.SetGlobal("__sailry", moejs.NativeFunc(func(_ *moejs.Realm, _ moejs.Value, arguments []moejs.Value) (moejs.Value, error) {
			operation, body := moejs.Arg(arguments, 0), moejs.Arg(arguments, 1)
			if !operation.IsString() || !body.IsString() {
				return moejs.Undefined(), scriptInvalid("invalid host SDK operation")
			}
			return moejs.String(bridge.call(operation.String(), body.String())), nil
		})); err != nil {
			return nil, runtime.engine(err)
		}
	}
	entry, err := json.Marshal(bundle.Entry)
	if err != nil {
		return nil, scriptEngine(err)
	}
	bootstrap, err := moejs.CompileWith("sailry:callback", "import * as plugin from "+string(entry)+";export {plugin};", moejs.CompileOptions{Context: bounded})
	if err != nil {
		return nil, runtime.engine(err)
	}
	linked, err := moejs.Link(bootstrap, resolve)
	if err != nil {
		return nil, runtime.engine(err)
	}
	if err := vm.Load(linked); err != nil {
		return nil, runtime.engine(err)
	}
	exports, exists := vm.Export("plugin")
	if !exists {
		return nil, scriptInvalid("plugin host entry is missing")
	}
	runtime.exports = exports
	if err := runtime.arm(); err != nil {
		return nil, err
	}
	return runtime, nil
}

func (runtime *scriptVM) arm() error {
	return scriptCheck(runtime.ctx, runtime.deadline, runtime.pure)
}

func (runtime *scriptVM) begin() (func(), error) {
	if err := scriptCheck(runtime.ctx, runtime.deadline, runtime.pure); err != nil {
		return nil, err
	}
	bounded, cancel := context.WithDeadline(runtime.ctx, runtime.deadline)
	interrupted := make(chan struct{})
	vm := runtime.vm
	stop := context.AfterFunc(bounded, func() {
		vm.Interrupt(context.Cause(bounded))
		close(interrupted)
	})
	return func() {
		if !stop() {
			<-interrupted
		}
		cancel()
	}, nil
}

func (runtime *scriptVM) settle(promise moejs.Value) (moejs.Value, error) {
	if err := runtime.arm(); err != nil {
		return moejs.Undefined(), err
	}
	state, value, valid := moejs.PromiseResult(promise)
	if !valid || state == moejs.PromisePending {
		if runtime.pure {
			return moejs.Undefined(), scriptInvalid("plugin translation failed")
		}
		return moejs.Undefined(), scriptInvalid("plugin JavaScript promise did not settle")
	}
	if state == moejs.PromiseRejected {
		return moejs.Undefined(), runtime.exception(value)
	}
	return value, nil
}

func (runtime *scriptVM) call(handler string, input any, maxBytes int) (json.RawMessage, error) {
	encoded, err := json.Marshal(input)
	if err != nil {
		return nil, scriptInvalid("plugin callback input is not JSON")
	}
	if len(encoded) > maxBytes {
		if runtime.pure {
			return nil, scriptInvalid("plugin translation input exceeds its byte budget")
		}
		return nil, scriptInvalid("plugin callback input exceeds limit")
	}
	stop, err := runtime.begin()
	if err != nil {
		return nil, err
	}
	defer stop()
	defer runtime.vm.ReleaseCallData()
	promise, err := runtime.vm.CallFunction(runtime.invoke, moejs.Undefined(), runtime.exports, moejs.String(handler), moejs.String(string(encoded)))
	if err != nil {
		return nil, runtime.engine(err)
	}
	value, err := runtime.settle(promise)
	if err != nil {
		return nil, err
	}
	if err := runtime.arm(); err != nil {
		return nil, err
	}
	text, err := runtime.vm.AppendJSON(nil, value)
	if err != nil {
		return nil, runtime.engine(err)
	}
	if err := scriptCheck(runtime.ctx, runtime.deadline, runtime.pure); err != nil {
		return nil, err
	}
	var envelope struct {
		Success bool            `json:"success"`
		Value   json.RawMessage `json:"value"`
		Error   scriptException `json:"error"`
	}
	if err := json.Unmarshal(text, &envelope); err != nil {
		return nil, scriptInvalid("plugin callback result is not JSON")
	}
	if !envelope.Success {
		return nil, runtime.fault(envelope.Error)
	}
	if len(envelope.Value) == 0 {
		return nil, scriptInvalid("plugin callback result is not JSON")
	}
	if len(envelope.Value) > maxBytes {
		if runtime.pure {
			return nil, scriptInvalid("plugin translation result exceeds its byte budget")
		}
		return nil, scriptInvalid("plugin callback result exceeds limit")
	}
	return envelope.Value, nil
}

func (runtime *scriptVM) close() {
	if runtime.releaseRoots != nil {
		runtime.releaseRoots()
		runtime.releaseRoots = nil
	}
	if runtime.vm != nil {
		runtime.vm.ReleaseCallData()
		runtime.vm = nil
	}
	runtime.exports, runtime.invoke, runtime.inspect, runtime.has = moejs.Undefined(), moejs.Undefined(), moejs.Undefined(), moejs.Undefined()
}

func scriptCheck(ctx context.Context, deadline time.Time, pure bool) error {
	if ctx.Err() != nil {
		message := "plugin callback cancelled"
		if pure {
			message = "plugin translation cancelled"
		}
		return &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: message}
	}
	if !time.Now().Before(deadline) {
		if pure {
			return &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "plugin translation deadline exceeded"}
		}
		return &protocol.Fault{Code: protocol.ErrorCodeExpired, Message: "plugin callback response timed out"}
	}
	return nil
}

func (runtime *scriptVM) engine(err error) error {
	if interrupted := scriptCheck(runtime.ctx, runtime.deadline, runtime.pure); interrupted != nil {
		return interrupted
	}
	if runtime.pure {
		return scriptInvalid("plugin translation failed")
	}
	if errors.Is(err, moejs.ErrModulePending) {
		return scriptInvalid("plugin JavaScript promise did not settle")
	}
	var exception *moejs.Exception
	if !errors.Is(err, moejs.ErrMemoryLimit) && errors.As(err, &exception) && runtime.inspect.IsObject() {
		return runtime.exception(exception.Value)
	}
	return scriptEngine(err)
}

func scriptEngine(err error) error {
	var fault *protocol.Fault
	if errors.As(err, &fault) {
		return fault
	}
	if errors.Is(err, moejs.ErrMemoryLimit) {
		return scriptInvalid("plugin JavaScript execution failed: out of memory")
	}
	var syntax *moejs.SyntaxError
	if errors.As(err, &syntax) && syntax.Message != "" && len(syntax.Message) <= 1024 {
		return scriptInvalid("plugin JavaScript execution failed: " + syntax.Message)
	}
	return scriptInvalid("plugin JavaScript execution failed")
}

func scriptInvalid(message string) *protocol.Fault {
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: message}
}

func scriptTranslationResource(name string) bool {
	if !strings.HasPrefix(name, "dev.sailry.platform/") || strings.Contains(name, "\\") {
		return false
	}
	for _, part := range strings.Split(name, "/") {
		if part == "" || part == "." || part == ".." {
			return false
		}
	}
	return true
}
