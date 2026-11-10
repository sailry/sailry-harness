package plugins

import (
	"context"
	"embed"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"path"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/MoeForge/moejs"
	"github.com/egoist/mygo/ui"
	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/protocol"
)

//go:embed js/*.js
var runtimeSources embed.FS

const (
	desktopLoadBudget  = 10 * time.Second
	desktopTaskBudget  = 5 * time.Second
	desktopFrameBudget = 2 * time.Second
)

type Runtime struct {
	bindings   RuntimeBindings
	modules    map[string]map[string]HostExport
	components map[string]NativeComponent
	mounts     map[*Mounted]struct{}
	clock      clockwork.Clock
	closed     bool
}

type Mounted struct {
	runtime    *Runtime
	engine     *moejs.Runtime
	bundle     Bundle
	context    PluginContext
	stop       context.Context
	cancel     context.CancelFunc
	closed     bool
	suspended  bool
	helper     moejs.Value
	modules    map[string]*moejs.Module
	execution  context.Context
	jobsHeld   bool
	jobsPosted bool
	unroot     func()
	transient  []moejs.Value
	next       uint64
	pending    map[uint64]*desktopPromise
	timers     map[uint64]clockwork.Timer
	queueMu    sync.Mutex
	queue      []runtimeCompletion
	failure    error
	last       *NativeElement
	inputs     map[uint64]*string
	toggles    map[uint64]*bool
	popovers   map[uint64]*bool
	selectors  map[uint64]*int
	widgetIDs  map[string]uint64
	nextWidget uint64
	viewport   map[string]float32
	nativeText map[uint64]string
	images     map[string]ui.ImageSource
}

type desktopPromise struct {
	value   moejs.Value
	resolve func(moejs.Value) error
	reject  func(moejs.Value) error
}

type runtimeCompletion struct {
	id    uint64
	data  json.RawMessage
	err   error
	timer bool
}

func NewRuntime(bindings RuntimeBindings) (*Runtime, error) {
	runtime := &Runtime{
		bindings: bindings, modules: map[string]map[string]HostExport{},
		components: map[string]NativeComponent{}, mounts: map[*Mounted]struct{}{},
		clock: clockwork.NewRealClock(),
	}
	for _, module := range bindings.Modules {
		if module.Name == "" || runtime.modules[module.Name] != nil || coreModule(module.Name) {
			return nil, fmt.Errorf("invalid or duplicate plugin host module %q", module.Name)
		}
		exports := map[string]HostExport{}
		for _, export := range module.Exports {
			if !runtimeIdentifier(export.Name) || (export.Sync == nil) == (export.Async == nil) {
				return nil, fmt.Errorf("invalid plugin host export %s.%s", module.Name, export.Name)
			}
			if _, exists := exports[export.Name]; exists {
				return nil, fmt.Errorf("duplicate plugin host export %s.%s", module.Name, export.Name)
			}
			exports[export.Name] = export
		}
		runtime.modules[module.Name] = exports
	}
	for _, component := range bindings.Components {
		key := component.Module + "/" + component.Export
		if !runtimeIdentifier(component.Export) || component.Module == "" || component.Render == nil {
			return nil, fmt.Errorf("invalid plugin native component %q", key)
		}
		if _, exists := runtime.components[key]; exists {
			return nil, fmt.Errorf("duplicate plugin native component %q", key)
		}
		runtime.components[key] = component
		if runtime.modules[component.Module] == nil {
			runtime.modules[component.Module] = map[string]HostExport{}
		}
		if _, exists := runtime.modules[component.Module][component.Export]; exists {
			return nil, fmt.Errorf("plugin component shadows host export %q", key)
		}
	}
	return runtime, nil
}

func (runtime *Runtime) SetClock(clock clockwork.Clock) error {
	if clock == nil || len(runtime.mounts) != 0 {
		return errors.New("plugin clock must be selected before mounting")
	}
	runtime.clock = clock
	return nil
}

func (runtime *Runtime) Mount(bundle Bundle, pluginContext PluginContext) (_ *Mounted, result error) {
	if runtime.closed {
		return nil, errors.New("plugin runtime is closed")
	}
	if err := protocol.ValidatePluginDesktopBundle(bundle); err != nil {
		return nil, err
	}
	if bundle.Package != pluginContext.Package {
		return nil, errors.New("plugin desktop bundle belongs to another package")
	}
	stop, cancel := context.WithCancel(context.Background())
	mounted := &Mounted{
		runtime: runtime, bundle: bundle, context: pluginContext, stop: stop, cancel: cancel,
		modules: map[string]*moejs.Module{}, pending: map[uint64]*desktopPromise{}, timers: map[uint64]clockwork.Timer{},
		inputs: map[uint64]*string{}, toggles: map[uint64]*bool{}, popovers: map[uint64]*bool{},
		selectors: map[uint64]*int{}, widgetIDs: map[string]uint64{},
		viewport: map[string]float32{"width": 1280, "height": 820}, nativeText: map[uint64]string{}, images: map[string]ui.ImageSource{},
	}
	mounted.engine = moejs.NewRuntime(moejs.Options{Importer: &moejs.Importer{Resolve: mounted.resolveModule}})
	mounted.unroot = mounted.engine.AddLiveRoots(func(add func(moejs.Value)) {
		add(mounted.helper)
		for _, promise := range mounted.pending {
			add(promise.value)
		}
		for _, value := range mounted.transient {
			add(value)
		}
	})
	defer func() {
		if result != nil {
			_ = mounted.Close()
		}
	}()
	if err := mounted.execute(desktopLoadBudget, func(ctx context.Context) error {
		if err := mounted.installBridge(ctx); err != nil {
			return err
		}
		encoded, err := json.Marshal(bundle.Entry)
		if err != nil {
			return err
		}
		entry, err := moejs.CompileWith("sailry:desktop", "globalThis.__desktopLoaded=false;import("+string(encoded)+").then(module=>{__desktop.mount(module.default);globalThis.__desktopLoaded=true;}).catch(error=>{__desktop.fail(error);});", moejs.CompileOptions{Context: ctx})
		if err != nil {
			return err
		}
		return mounted.engine.Load(entry)
	}); err != nil {
		return nil, err
	}
	if err := mounted.Pump(); err != nil {
		return nil, err
	}
	runtime.mounts[mounted] = struct{}{}
	return mounted, nil
}

func (mounted *Mounted) installBridge(ctx context.Context) error {
	functions := map[string]moejs.NativeFunc{
		"__desktop_host":        mounted.host,
		"__desktop_async_value": mounted.async,
		"__desktop_timer":       mounted.schedule,
		"__desktop_cancel_timer": func(_ *moejs.Realm, _ moejs.Value, arguments []moejs.Value) (moejs.Value, error) {
			identifier := runtimeUint(arguments, 0)
			if timer := mounted.timers[identifier]; timer != nil {
				timer.Stop()
				delete(mounted.timers, identifier)
			}
			return moejs.Undefined(), nil
		},
		"__desktop_notify": func(_ *moejs.Realm, _ moejs.Value, _ []moejs.Value) (moejs.Value, error) {
			if mounted.runtime.bindings.Invalidate != nil {
				mounted.runtime.bindings.Invalidate()
			}
			return moejs.Undefined(), nil
		},
	}
	for name, function := range functions {
		if err := mounted.engine.SetGlobal(name, function); err != nil {
			return err
		}
	}
	for _, name := range []string{"bridge", "gpui-prelude", "core"} {
		source, err := runtimeSources.ReadFile("js/" + name + ".js")
		if err != nil {
			return err
		}
		script, err := moejs.CompileScriptWith("sailry:"+name, string(source), moejs.CompileOptions{Context: ctx})
		if err != nil {
			return fmt.Errorf("plugin runtime %s: %w", name, err)
		}
		value, err := mounted.engine.RunScript(script)
		if err != nil {
			return fmt.Errorf("plugin runtime %s: %w", name, err)
		}
		if name == "bridge" {
			mounted.helper = value
		}
	}
	components := []string{}
	for key := range mounted.runtime.components {
		components = append(components, key)
	}
	if _, err := mounted.call("configure", components); err != nil {
		return err
	}
	return nil
}

func (mounted *Mounted) host(_ *moejs.Realm, _ moejs.Value, arguments []moejs.Value) (moejs.Value, error) {
	if mounted.closed || len(arguments) != 3 {
		return moejs.Undefined(), errors.New("plugin host is closed or the call is invalid")
	}
	module, name, data := arguments[0].String(), arguments[1].String(), arguments[2].String()
	var values []json.RawMessage
	if err := json.Unmarshal([]byte(data), &values); err != nil {
		return moejs.Undefined(), err
	}
	operation := mounted.runtime.modules[module][name].Sync
	if operation == nil {
		return moejs.Undefined(), fmt.Errorf("plugin synchronous export %s.%s is unavailable", module, name)
	}
	result, err := operation(mounted.execution, values)
	if err != nil {
		return moejs.Undefined(), err
	}
	return mounted.hostData(result)
}

func (mounted *Mounted) async(_ *moejs.Realm, _ moejs.Value, arguments []moejs.Value) (moejs.Value, error) {
	promise, resolve, reject := mounted.engine.NewPromise()
	fail := func(failure error) (moejs.Value, error) {
		reason, _ := mounted.engine.ThrownValue(failure)
		return promise, reject(reason)
	}
	if mounted.closed || len(arguments) != 3 {
		return fail(errors.New("plugin host is closed or the call is invalid"))
	}
	module, name, data := arguments[0].String(), arguments[1].String(), arguments[2].String()
	var values []json.RawMessage
	if err := json.Unmarshal([]byte(data), &values); err != nil {
		return fail(err)
	}
	operation := mounted.runtime.modules[module][name].Async
	if operation == nil {
		return fail(fmt.Errorf("plugin asynchronous export %s.%s is unavailable", module, name))
	}
	mounted.next++
	identifier := mounted.next
	mounted.pending[identifier] = &desktopPromise{value: promise, resolve: resolve, reject: reject}
	go func() {
		result, err := operation(mounted.stop, values)
		mounted.enqueue(runtimeCompletion{id: identifier, data: result, err: err})
	}()
	return promise, nil
}

func (mounted *Mounted) schedule(_ *moejs.Realm, _ moejs.Value, arguments []moejs.Value) (moejs.Value, error) {
	if mounted.closed || len(arguments) != 2 {
		return moejs.Undefined(), errors.New("invalid plugin timer")
	}
	identifier := runtimeUint(arguments, 0)
	if !arguments[1].IsNumber() {
		return moejs.Undefined(), errors.New("plugin timer delay must be a number")
	}
	milliseconds := arguments[1].AsNumber()
	if math.IsNaN(milliseconds) || math.IsInf(milliseconds, 0) || milliseconds < 0 || milliseconds >= float64(time.Duration(1<<63-1))/float64(time.Millisecond) {
		return moejs.Undefined(), errors.New("plugin timer delay is outside the supported duration range")
	}
	mounted.timers[identifier] = mounted.runtime.clock.AfterFunc(time.Duration(milliseconds*float64(time.Millisecond)), func() {
		mounted.enqueue(runtimeCompletion{id: identifier, timer: true})
	})
	return moejs.Undefined(), nil
}

func (mounted *Mounted) enqueue(completion runtimeCompletion) {
	mounted.queueMu.Lock()
	if mounted.stop.Err() == nil {
		mounted.queue = append(mounted.queue, completion)
	}
	mounted.queueMu.Unlock()
	if mounted.stop.Err() == nil && mounted.runtime.bindings.Post != nil {
		mounted.runtime.bindings.Post(func() {
			if !mounted.closed {
				if err := mounted.Pump(); err != nil {
					mounted.failure = err
				}
			}
		})
	}
	if mounted.stop.Err() == nil && mounted.runtime.bindings.Invalidate != nil {
		mounted.runtime.bindings.Invalidate()
	}
}

func (mounted *Mounted) Pump() error {
	return mounted.pump(true)
}

func (mounted *Mounted) pump(releaseJobs bool) error {
	if mounted.closed {
		return errors.New("plugin view is closed")
	}
	if mounted.suspended {
		return nil
	}
	if mounted.failure != nil {
		return mounted.failure
	}
	return mounted.execute(desktopTaskBudget, func(context.Context) error {
		if releaseJobs && mounted.jobsHeld {
			mounted.jobsHeld = false
			if err := mounted.engine.Realm().ReleaseJobs(nil); err != nil {
				return err
			}
		}
		mounted.queueMu.Lock()
		completions := mounted.queue
		mounted.queue = nil
		mounted.queueMu.Unlock()
		for _, completion := range completions {
			if completion.timer {
				delete(mounted.timers, completion.id)
				if _, err := mounted.call("timer", completion.id); err != nil {
					return err
				}
				continue
			}
			promise := mounted.pending[completion.id]
			if promise == nil {
				continue
			}
			var settleErr error
			if completion.err != nil {
				reason, _ := mounted.engine.ThrownValue(completion.err)
				settleErr = promise.reject(reason)
			} else {
				data, err := mounted.hostData(completion.data)
				if err != nil {
					reason, _ := mounted.engine.ThrownValue(err)
					settleErr = promise.reject(reason)
				} else {
					settleErr = promise.resolve(data)
				}
			}
			delete(mounted.pending, completion.id)
			if settleErr != nil {
				return settleErr
			}
		}
		_, err := mounted.call("check")
		return err
	})
}

func (mounted *Mounted) call(name string, arguments ...any) (any, error) {
	budget := desktopTaskBudget
	if name == "render" {
		budget = desktopFrameBudget
	}
	var result any
	err := mounted.execute(budget, func(context.Context) error {
		base := len(mounted.transient)
		defer func() {
			clear(mounted.transient[base:])
			mounted.transient = mounted.transient[:base]
		}()
		function, err := mounted.engine.Get(mounted.helper, name)
		if err != nil {
			return err
		}
		mounted.transient = append(mounted.transient, function)
		values := make([]moejs.Value, 0, len(arguments))
		for _, argument := range arguments {
			value, err := mounted.engine.FromGo(argument)
			if err != nil {
				return err
			}
			values = append(values, value)
			mounted.transient = append(mounted.transient, value)
		}
		if name == "render" && !mounted.jobsHeld {
			mounted.engine.Realm().HoldJobs()
			mounted.jobsHeld = true
		}
		value, err := mounted.engine.CallFunction(function, mounted.helper, values...)
		if err != nil {
			return err
		}
		mounted.transient = append(mounted.transient, value)
		result, err = mounted.engine.ToGo(value)
		if name == "render" && !mounted.engine.Realm().JobsPending() {
			mounted.jobsHeld = false
			err = mounted.engine.Realm().ReleaseJobs(err)
		}
		return err
	})
	return result, err
}

func (mounted *Mounted) Snapshot() (*NativeElement, error) {
	if mounted.jobsHeld {
		if err := mounted.Pump(); err != nil {
			return nil, err
		}
	}
	return mounted.snapshot()
}

func (mounted *Mounted) snapshot() (*NativeElement, error) {
	if mounted.closed {
		return nil, errors.New("plugin view is closed")
	}
	result, err := mounted.call("render")
	if err != nil {
		return nil, err
	}
	text, ok := result.(string)
	if !ok {
		return nil, errors.New("plugin render returned no element snapshot")
	}
	var element NativeElement
	if err := json.Unmarshal([]byte(text), &element); err != nil {
		return nil, err
	}
	mounted.last = &element
	if mounted.jobsHeld && !mounted.jobsPosted && mounted.runtime.bindings.Post != nil {
		mounted.jobsPosted = true
		mounted.runtime.bindings.Post(func() {
			mounted.jobsPosted = false
			if !mounted.closed {
				if err := mounted.Pump(); err != nil {
					mounted.failure = err
				}
			}
		})
	}
	return &element, nil
}

func (mounted *Mounted) Dispatch(callback CallbackRef, event json.RawMessage) error {
	if mounted.closed || mounted.suspended {
		return errors.New("plugin view is closed")
	}
	var data any
	if len(event) != 0 {
		if err := json.Unmarshal(event, &data); err != nil {
			return err
		}
	}
	if _, err := mounted.call("dispatch", callback.Generation, callback.ID, data); err != nil {
		return err
	}
	return mounted.Pump()
}

func (mounted *Mounted) Context() PluginContext {
	return mounted.context
}

func (mounted *Mounted) Pending() int {
	return len(mounted.pending)
}

func (mounted *Mounted) Error() error {
	return mounted.failure
}

func (mounted *Mounted) Suspended() bool {
	return mounted.suspended
}

func (mounted *Mounted) Suspend() {
	if mounted.closed || mounted.suspended {
		return
	}
	mounted.suspended = true
	mounted.cancel()
	for _, timer := range mounted.timers {
		timer.Stop()
	}
	clear(mounted.timers)
	mounted.pending = nil
	mounted.queueMu.Lock()
	mounted.queue = nil
	mounted.queueMu.Unlock()
	if mounted.unroot != nil {
		mounted.unroot()
		mounted.unroot = nil
	}
	mounted.helper = moejs.Undefined()
	mounted.transient = nil
	mounted.modules = nil
	mounted.engine = nil
}

func (mounted *Mounted) Close() error {
	if mounted.closed {
		return nil
	}
	mounted.Suspend()
	mounted.closed = true
	mounted.last = nil
	delete(mounted.runtime.mounts, mounted)
	return nil
}

func (runtime *Runtime) Close() error {
	if runtime.closed {
		return nil
	}
	runtime.closed = true
	var result error
	for mounted := range runtime.mounts {
		result = errors.Join(result, mounted.Close())
	}
	return result
}

func (runtime *Runtime) moduleSource(name string) string {
	var source strings.Builder
	names := []string{}
	for name := range runtime.modules[name] {
		names = append(names, name)
	}
	sort.Strings(names)
	module, _ := json.Marshal(name)
	for _, export := range names {
		encoded, _ := json.Marshal(export)
		if runtime.modules[name][export].Async != nil {
			fmt.Fprintf(&source, "export const %s=(...args)=>__desktop_async(%s,%s,JSON.stringify(args));\n", export, module, encoded)
		} else {
			fmt.Fprintf(&source, "export const %s=(...args)=>__desktop_host(%s,%s,JSON.stringify(args));\n", export, module, encoded)
		}
	}
	for _, component := range runtime.bindings.Components {
		if component.Module == name {
			export, _ := json.Marshal(component.Export)
			fmt.Fprintf(&source, "export const %s=__gpui.component(%s,%s);\n", component.Export, module, export)
		}
	}
	return source.String()
}

func (mounted *Mounted) hostData(data json.RawMessage) (moejs.Value, error) {
	if len(data) == 0 {
		return moejs.Null(), nil
	}
	return mounted.engine.ParseJSON(data)
}

func runtimeIdentifier(name string) bool {
	if name == "" {
		return false
	}
	for index, character := range name {
		if character != '_' && character != '$' && (character < 'a' || character > 'z') && (character < 'A' || character > 'Z') && (index == 0 || character < '0' || character > '9') {
			return false
		}
	}
	return true
}

func runtimeUint(arguments []moejs.Value, index int) uint64 {
	if index >= len(arguments) || !arguments[index].IsNumber() {
		return 0
	}
	return uint64(arguments[index].AsNumber())
}

func (mounted *Mounted) resolveModule(referrer moejs.Referrer, name string) (*moejs.Module, error) {
	if strings.HasPrefix(name, ".") && referrer != nil {
		name = path.Join(path.Dir(referrer.Name()), name)
	}
	if module := mounted.modules[name]; module != nil {
		return module, nil
	}
	var source string
	switch {
	case coreModule(name):
		source = coreModuleSource(name)
	case mounted.runtime.modules[name] != nil:
		source = mounted.runtime.moduleSource(name)
	default:
		var exists bool
		source, exists = mounted.bundle.Files[name]
		if !exists || strings.HasPrefix(name, "../") {
			return nil, fmt.Errorf("plugin module %q is outside its declared resources", name)
		}
	}
	module, err := moejs.CompileWith(name, source, moejs.CompileOptions{Context: mounted.execution})
	if err != nil {
		return nil, err
	}
	mounted.modules[name] = module
	return module, nil
}

func (mounted *Mounted) execute(budget time.Duration, body func(context.Context) error) (result error) {
	if mounted.execution != nil {
		return body(mounted.execution)
	}
	if mounted.engine == nil {
		return errors.New("plugin view is closed")
	}
	engine := mounted.engine
	ctx, cancel := context.WithTimeout(mounted.stop, budget)
	interrupted := make(chan struct{})
	stop := context.AfterFunc(ctx, func() {
		engine.Interrupt(context.Cause(ctx))
		close(interrupted)
	})
	mounted.execution = ctx
	defer func() {
		mounted.execution = nil
		if !stop() {
			<-interrupted
		}
		if result == nil && ctx.Err() != nil {
			result = ctx.Err()
		}
		cancel()
		var interruption *moejs.InterruptedError
		if errors.As(result, &interruption) && mounted.jobsHeld {
			mounted.engine.Realm().DropJobs(result)
			mounted.jobsHeld = false
			result = mounted.engine.Realm().ReleaseJobs(result)
		}
		engine.ClearInterrupt()
		if !mounted.jobsHeld {
			result = errors.Join(result, engine.RebaseAllocation(nil))
			engine.ReleaseCallData()
		}
	}()
	return body(ctx)
}
