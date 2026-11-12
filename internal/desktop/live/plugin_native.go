package live

import (
	"context"
	"encoding/json"
	"fmt"
	"slices"
	"sync"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginNativeState struct {
	runtime       *Runtime
	scope         protocol.PluginContext
	presentation  string
	native        *plugins.NativeBinding
	mu            sync.Mutex
	palette       *theme.Palette
	resource      *attachmentDescriptor
	artifactViews map[string]*pluginNativeArtifact
	artifactWatch *pluginNativeArtifactWatch
	terminals     map[string]*pluginNativeTerminal
	conversations map[string]*pluginNativeConversation
	terminalFocus string
	toasts        map[string]uint64
	toastSequence uint64
	toastEvents   chan json.RawMessage
	composer      *pluginNativeComposer
	providers     *pluginNativeProviders
}

func (r *Runtime) pluginNativeBinding(ctx context.Context, scope protocol.PluginContext, presentation string) *plugins.NativeBinding {
	return r.pluginNativeResourceBinding(ctx, scope, presentation, nil)
}

func (r *Runtime) pluginNativeResourceBinding(ctx context.Context, scope protocol.PluginContext, presentation string, resource *attachmentDescriptor) *plugins.NativeBinding {
	if resource != nil {
		captured := *resource
		if captured.artifact != nil {
			artifact := *captured.artifact
			captured.artifact = &artifact
		}
		resource = &captured
	}
	binding := plugins.NewNativeBinding(ctx)
	state := &pluginNativeState{
		runtime: r, scope: scope, presentation: presentation, native: &binding, resource: resource,
		palette:       theme.Look(r.App.Theme, r.App.Mode == theme.Dark),
		terminals:     map[string]*pluginNativeTerminal{},
		conversations: map[string]*pluginNativeConversation{},
		artifactViews: map[string]*pluginNativeArtifact{},
		artifactWatch: &pluginNativeArtifactWatch{changed: make(chan struct{})},
		toasts:        map[string]uint64{}, toastEvents: make(chan json.RawMessage, 16),
	}
	binding.Post = state.post
	binding.Invalidate = r.localRuntime().wake
	binding.Theme = func() json.RawMessage {
		state.mu.Lock()
		palette := state.palette
		state.mu.Unlock()
		return plugins.ThemeSnapshot(palette)
	}
	binding.TerminalAppearance = func() (protocol.TerminalAppearance, error) {
		if err := binding.Context.Err(); err != nil {
			return protocol.TerminalAppearance{}, err
		}
		state.mu.Lock()
		palette := state.palette
		state.mu.Unlock()
		return pluginNativeAppearance(palette), nil
	}
	binding.ChooseSecretFile = state.chooseSecretFile
	binding.Operations = map[string]plugins.HostOperation{}
	state.navigationOperations()
	state.browserOperations()
	state.fileOperations()
	state.artifactOperations()
	state.terminalOperations()
	state.composerOperations()
	state.providerOperations()
	binding.Frame = func(view *ui.Context) {
		state.mu.Lock()
		state.palette = theme.Of(view)
		state.mu.Unlock()
		state.refreshComposer()
		state.refreshProviders()
	}
	state.locationOperations()
	state.sshTransferOperations()
	binding.Release = func() {
		for identity, mounted := range state.terminals {
			mounted.release()
			delete(state.terminals, identity)
		}
		for identity, mounted := range state.conversations {
			state.releaseConversation(mounted)
			delete(state.conversations, identity)
		}
		clear(state.artifactViews)
	}
	binding.Components = []plugins.NativeComponent{
		{Module: "sailry", Export: "Terminal", Render: state.renderTerminal},
		{Module: "sailry", Export: "Conversation", Render: state.renderConversation},
		{Module: "sailry/artifacts", Export: "ArtifactSurface", Render: state.renderArtifact},
	}
	context.AfterFunc(binding.Context, func() {
		state.mu.Lock()
		toasts := make([]string, 0, len(state.toasts))
		for id := range state.toasts {
			toasts = append(toasts, state.toastID(id))
		}
		clear(state.toasts)
		state.mu.Unlock()
		r.enqueue(func(view *ui.Context) {
			for _, id := range toasts {
				view.CloseToast(id)
			}
		})
	})
	return &binding
}

func (state *pluginNativeState) post(apply func(*ui.Context)) bool {
	if state.native.Context.Err() != nil || state.runtime.ctx.Err() != nil {
		return false
	}
	root := state.runtime.localRuntime()
	root.mu.Lock()
	if root.closed {
		root.mu.Unlock()
		return false
	}
	root.queue = append(root.queue, func(view *ui.Context) {
		if state.native.Context.Err() == nil && state.runtime.ctx.Err() == nil {
			state.mu.Lock()
			state.palette = theme.Of(view)
			state.mu.Unlock()
			apply(view)
		}
	})
	root.mu.Unlock()
	root.wake()
	return true
}

func (state *pluginNativeState) update(ctx context.Context, apply func(*ui.Context) (json.RawMessage, error)) (json.RawMessage, error) {
	type result struct {
		value json.RawMessage
		err   error
	}
	reply := make(chan result, 1)
	if !state.post(func(view *ui.Context) {
		if ctx.Err() != nil {
			reply <- result{err: ctx.Err()}
			return
		}
		value, err := apply(view)
		reply <- result{value: value, err: err}
	}) {
		return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "plugin view is closed")
	}
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-state.native.Context.Done():
		return nil, state.native.Context.Err()
	case result := <-reply:
		return result.value, result.err
	}
}

func (state *pluginNativeState) effect(apply func(*ui.Context)) (json.RawMessage, error) {
	if !state.post(apply) {
		return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "plugin view is closed")
	}
	return json.RawMessage("null"), nil
}

func (state *pluginNativeState) execute(ctx context.Context, command protocol.Command) (protocol.Output, error) {
	request := state.runtime.controller.Prepare(command)
	scope := state.scope
	request.Plugin = &scope
	return state.runtime.controller.Execute(ctx, request)
}

func (state *pluginNativeState) packageInfo() (protocol.PluginInfo, bool) {
	if state.native.Context.Err() != nil || state.runtime.packages == nil {
		return protocol.PluginInfo{}, false
	}
	info, exists := state.runtime.packages.infos[state.scope.Package.Name]
	return info, exists && pluginNativePackageAvailable(info) && packageReference(info.Summary) == state.scope.Package
}

func (state *pluginNativeState) workspaceComponent(actions ...protocol.PluginAction) error {
	info, available := state.packageInfo()
	if !available {
		return pluginNativeFault(protocol.ErrorCodeUnavailable, "plugin native scope is unavailable")
	}
	if state.scope.Surface.Kind != "workspace" || info.Extension == nil {
		return pluginNativeFault(protocol.ErrorCodePermissionDenied, "native component requires a workspace surface")
	}
	for _, action := range actions {
		if !slices.Contains(info.Extension.Actions, action) {
			return pluginNativeFault(protocol.ErrorCodePermissionDenied, "native component action is not declared")
		}
	}
	return nil
}

func pluginNativeFault(code protocol.ErrorCode, message string) error {
	return &protocol.Fault{Code: code, Message: message}
}

func pluginNativeJSON(value any) (json.RawMessage, error) {
	return json.Marshal(value)
}

func pluginNativeArgument[Value any](arguments []json.RawMessage, index int) (Value, error) {
	var value Value
	if index >= len(arguments) {
		return value, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "missing plugin native argument")
	}
	if err := json.Unmarshal(arguments[index], &value); err != nil {
		return value, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid plugin native argument")
	}
	return value, nil
}

func pluginNativeProps(element plugins.NativeElement) (string, map[string]json.RawMessage, error) {
	identity := fmt.Sprintf("native-%d", element.ID)
	props := map[string]json.RawMessage{}
	if len(element.Props) == 0 || string(element.Props) == "null" {
		return identity, props, nil
	}
	var arguments []json.RawMessage
	if json.Unmarshal(element.Props, &arguments) == nil {
		var err error
		identity, err = pluginNativeArgument[string](arguments, 0)
		if err != nil {
			return "", nil, err
		}
		if len(arguments) > 1 && string(arguments[1]) != "null" {
			err = json.Unmarshal(arguments[1], &props)
		}
		return identity, props, err
	}
	if err := json.Unmarshal(element.Props, &props); err != nil {
		return "", nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid native component props")
	}
	if value := props["id"]; len(value) > 0 {
		_ = json.Unmarshal(value, &identity)
	}
	return identity, props, nil
}

func pluginNativeAppearance(palette *theme.Palette) protocol.TerminalAppearance {
	color := func(value ui.Color) protocol.TerminalRgb {
		return protocol.TerminalRgb{Red: value.R, Green: value.G, Blue: value.B}
	}
	value := protocol.TerminalAppearance{
		Foreground: color(palette.Foreground), Background: color(palette.Background),
		ColorScheme: protocol.TerminalColorSchemeLight,
	}
	if palette.Dark {
		value.ColorScheme = protocol.TerminalColorSchemeDark
	}
	for index, entry := range []ui.Color{
		palette.Background, palette.Danger, palette.Green, palette.Yellow,
		palette.Blue, palette.Magenta, palette.Info, palette.Foreground,
		palette.MutedForeground, palette.Danger, palette.Green, palette.Yellow,
		palette.Blue, palette.Magenta, palette.Info, palette.Foreground,
	} {
		value.Palette[index] = color(entry)
	}
	return value
}
