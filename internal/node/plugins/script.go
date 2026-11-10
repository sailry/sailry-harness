package plugins

import (
	"context"
	"embed"
	"encoding/json"
	"errors"
	"path"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
	"modernc.org/quickjs"
)

const ScriptMemoryBytes = 64 * 1024 * 1024

// ScriptStackSlots is the Go binding's fixed-slot counterpart of Rust STACK_BYTES.
const ScriptStackSlots = 512

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
}

type scriptVM struct {
	vm       *quickjs.VM
	ctx      context.Context
	deadline time.Time
	exports  quickjs.Value
	invoke   quickjs.Value
	inspect  quickjs.Value
	has      quickjs.Value
	watch    quickjs.Value
	done     quickjs.Atom
	failed   quickjs.Atom
	value    quickjs.Atom
	bridge   *scriptBridge
	pure     bool
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
	vm, err := quickjs.NewVM()
	if err != nil {
		return nil, scriptEngine(err)
	}
	runtime := &scriptVM{vm: vm, ctx: ctx, deadline: deadline, bridge: bridge, pure: pure}
	defer func() {
		if result != nil {
			runtime.close()
		}
	}()
	vm.SetMemoryLimit(ScriptMemoryBytes)
	vm.SetMaxStackSize(ScriptStackSlots)
	if err := runtime.arm(); err != nil {
		return nil, err
	}
	intrinsics, err := vm.EvalValue(scriptIntrinsics, quickjs.EvalGlobal)
	if err != nil {
		return nil, scriptEngine(err)
	}
	defer intrinsics.Free()
	for name, target := range map[string]*quickjs.Value{
		"invoke": &runtime.invoke, "inspect": &runtime.inspect, "has": &runtime.has, "watch": &runtime.watch,
	} {
		atom, err := vm.NewAtom(name)
		if err != nil {
			return nil, scriptEngine(err)
		}
		value, err := intrinsics.GetPropertyValue(atom)
		if err != nil {
			return nil, scriptEngine(err)
		}
		*target = value
	}
	for name, target := range map[string]*quickjs.Atom{
		"done": &runtime.done, "failed": &runtime.failed, "value": &runtime.value,
	} {
		atom, err := vm.NewAtom(name)
		if err != nil {
			return nil, scriptEngine(err)
		}
		*target = atom
	}
	if bridge != nil {
		if err := vm.RegisterFunc("__sailry", bridge.call, false); err != nil {
			return nil, scriptEngine(err)
		}
	}
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
	vm.SetModuleLoader(func(_ *quickjs.VM, name string) (string, error) {
		if err := scriptCheck(ctx, runtime.deadline, pure); err != nil {
			vm.Interrupt()
			return "", err
		}
		if source, ok := builtins[name]; ok {
			return source, nil
		}
		if source, ok := bundle.Files[name]; ok {
			return source, nil
		}
		return "", scriptInvalid("plugin module is unavailable")
	}, func(_ *quickjs.VM, base, name string) (string, error) {
		if strings.HasPrefix(name, ".") {
			name = path.Join(path.Dir(base), name)
		}
		if _, ok := builtins[name]; ok {
			return name, nil
		}
		if _, ok := bundle.Files[name]; ok {
			return name, nil
		}
		return "", scriptInvalid("plugin module is unavailable")
	})
	entry, err := json.Marshal(bundle.Entry)
	if err != nil {
		return nil, scriptEngine(err)
	}
	stop := context.AfterFunc(ctx, vm.Interrupt)
	defer stop()
	if err := runtime.arm(); err != nil {
		return nil, err
	}
	ready, err := vm.EvalValue("import("+string(entry)+")", quickjs.EvalGlobal)
	if err != nil {
		return nil, runtime.engine(err)
	}
	defer ready.Free()
	exports, err := runtime.settle(ready)
	if err != nil {
		return nil, err
	}
	runtime.exports = exports
	return runtime, nil
}

func (runtime *scriptVM) arm() error {
	if err := scriptCheck(runtime.ctx, runtime.deadline, runtime.pure); err != nil {
		return err
	}
	return runtime.vm.SetEvalTimeout(time.Until(runtime.deadline))
}

func (runtime *scriptVM) settle(promise quickjs.Value) (quickjs.Value, error) {
	if err := runtime.arm(); err != nil {
		return quickjs.Value{}, err
	}
	state, err := runtime.watch.CallValue(quickjs.UndefinedValue, promise)
	if err != nil {
		return quickjs.Value{}, runtime.engine(err)
	}
	defer state.Free()
	for {
		done, err := state.Property(runtime.done)
		if err != nil {
			return quickjs.Value{}, runtime.engine(err)
		}
		if done == true {
			break
		}
		if err := runtime.arm(); err != nil {
			return quickjs.Value{}, err
		}
		jobs, err := runtime.vm.ExecutePendingJobs()
		if err != nil {
			return quickjs.Value{}, runtime.engine(err)
		}
		if jobs == 0 {
			if runtime.pure {
				return quickjs.Value{}, scriptInvalid("plugin translation failed")
			}
			return quickjs.Value{}, scriptInvalid("plugin JavaScript promise did not settle")
		}
	}
	if err := scriptCheck(runtime.ctx, runtime.deadline, runtime.pure); err != nil {
		return quickjs.Value{}, err
	}
	failed, err := state.Property(runtime.failed)
	if err != nil {
		return quickjs.Value{}, runtime.engine(err)
	}
	value, err := state.GetPropertyValue(runtime.value)
	if err != nil {
		return quickjs.Value{}, runtime.engine(err)
	}
	if failed == true {
		defer value.Free()
		return quickjs.Value{}, runtime.exception(value)
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
	if err := runtime.arm(); err != nil {
		return nil, err
	}
	stop := context.AfterFunc(runtime.ctx, runtime.vm.Interrupt)
	defer stop()
	promise, err := runtime.invoke.CallValue(quickjs.UndefinedValue, runtime.exports, handler, string(encoded))
	if err != nil {
		return nil, runtime.engine(err)
	}
	defer promise.Free()
	value, err := runtime.settle(promise)
	if err != nil {
		return nil, err
	}
	defer value.Free()
	if err := runtime.arm(); err != nil {
		return nil, err
	}
	text, err := value.MarshalJSON()
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
	for _, value := range []*quickjs.Value{&runtime.exports, &runtime.invoke, &runtime.inspect, &runtime.has, &runtime.watch} {
		if value.VM() != nil {
			value.Free()
		}
	}
	if runtime.vm != nil {
		_ = runtime.vm.Close()
		runtime.vm = nil
	}
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
	return scriptEngine(err)
}

func scriptEngine(err error) error {
	var fault *protocol.Fault
	if errors.As(err, &fault) {
		return fault
	}
	var engine *quickjs.Error
	if errors.As(err, &engine) && engine.Message != "" && len(engine.Message) <= 1024 {
		return scriptInvalid("plugin JavaScript execution failed: " + engine.Message)
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
