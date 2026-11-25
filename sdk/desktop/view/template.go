package view

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"slices"
	"sync"
	"sync/atomic"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type (
	Operation = plugins.HostOperation
	Module    = plugins.HostModule
	Export    = plugins.HostExport
)

type Template struct {
	Entry string
	Files map[string]string
}

type Resources struct {
	Credential   func(string) (protocol.Secret, error)
	DroppedFiles func(string) ([]string, error)
	Launch       func() (any, error)
}

type Component struct {
	Module string
	Name   string
	Render func(*ui.Context, map[string]json.RawMessage, bool) (ui.Element, func(*ui.Context), error)
}

type Registration struct {
	Exports    []Module
	Components []Component
	CloseTab   string
	// Release frees what the registration mounted when the template closes.
	Release func()
}

type Options struct {
	Services    Services
	Template    Template
	Preferences string
	Builtins    map[string][]string
	Register    func(Resources) Registration
}

type Mount struct {
	ctx         context.Context
	cancel      context.CancelFunc
	mu          sync.Mutex
	services    Services
	locale      string
	preferences string
	active      *ui.Context
	// launch is the terminal launch measured in the latest content frame.
	// Scripts prepare terminals between frames, without a drawing context.
	launch   atomic.Value
	host     *plugins.Host
	engine   *plugins.Runtime
	view     *plugins.Mounted
	widgets  plugins.NativeBinding
	tabs     plugins.NativeComponent
	title    header
	left     func(*ui.Context) error
	right    func(*ui.Context) error
	closeTab string
	release  func()
	closed   bool
	err      error
}

func Open(ctx context.Context, c *ui.Context, options Options) (_ *Mount, err error) {
	ctx, cancel := context.WithCancel(ctx)
	s := options.Services
	m := &Mount{ctx: ctx, cancel: cancel, services: s, preferences: options.Preferences, locale: c.Locale(), active: c}
	m.measureLaunch(c)
	m.widgets = plugins.NewNativeBinding(ctx)
	m.widgets.Post, m.widgets.Invalidate, m.widgets.ChooseSecretFile = s.Post, s.Wake, s.PickSecret
	m.widgets.Theme = func() json.RawMessage {
		if m.active != nil {
			return plugins.ThemeSnapshot(theme.Of(m.active))
		}
		return json.RawMessage("null")
	}
	m.widgets.Operations = map[string]plugins.HostOperation{
		"sailry/ui.writeClipboard": func(_ context.Context, args []json.RawMessage) (json.RawMessage, error) {
			var value string
			if len(args) != 1 || json.Unmarshal(args[0], &value) != nil || s.WriteClipboard == nil {
				return nil, errors.New("clipboard is unavailable")
			}
			return json.RawMessage("null"), s.WriteClipboard(value)
		},
		"sailry/ui.toast": m.toast,
	}
	m.widgets.Header = func(raw json.RawMessage) error { return json.Unmarshal(raw, &m.title) }
	m.widgets.Navigation = func(draw func(*ui.Context) error) { m.left = draw }
	m.widgets.Details = func(draw func(*ui.Context) error) {
		changed := (m.right == nil) != (draw == nil)
		m.right = draw
		if changed {
			s.Wake()
		}
	}
	m.host = plugins.NewHost(plugins.HostBinding{Context: ctx, Native: &m.widgets, Locale: m.locale})
	defer func() {
		m.active = nil
		if err != nil {
			m.Close()
		}
	}()
	registration := options.Register(Resources{
		Credential: m.widgets.ResolveCredential, DroppedFiles: m.widgets.TakeDroppedFiles,
		Launch: func() (any, error) {
			if launch, ok := m.launch.Load().(launchValue); ok {
				return launch.value, nil
			}
			return nil, errors.New("terminal launch is unavailable")
		},
	})
	m.closeTab = registration.CloseTab
	m.release = registration.Release
	for _, component := range registration.Components {
		m.widgets.Components = append(m.widgets.Components, plugins.NativeComponent{
			Module: component.Module, Export: component.Name, Render: m.component(component),
		})
	}
	modules, err := m.modules(options.Builtins, append([]Module{m.contextModule()}, registration.Exports...))
	if err != nil {
		return nil, err
	}
	components, err := plugins.NativeComponents(m.widgets)
	if err != nil {
		return nil, err
	}
	for _, component := range components {
		if component.Module == "sailry/ui" && component.Export == "NavigationTabs" {
			m.tabs = component
			break
		}
	}
	m.engine, err = plugins.NewRuntime(plugins.RuntimeBindings{
		Modules: modules, Components: components, Invalidate: s.Wake, BindActions: m.widgets.BindActions,
		Post: func(operation func()) {
			s.Post(func(c *ui.Context) { m.frame(c, func(*ui.Context) error { operation(); return nil }) })
		},
	})
	if err != nil {
		return nil, err
	}
	m.view, err = m.engine.Mount(plugins.Bundle{Entry: options.Template.Entry, Files: options.Template.Files}, plugins.PluginContext{})
	return m, err
}

func (m *Mount) modules(builtins map[string][]string, extra []Module) ([]Module, error) {
	available, err := m.host.Modules()
	if err != nil {
		return nil, err
	}
	modules := make(map[string][]Export, len(extra))
	for _, module := range available {
		for _, export := range module.Exports {
			if slices.Contains(builtins[module.Name], export.Name) {
				modules[module.Name] = append(modules[module.Name], export)
			}
		}
	}
	for _, module := range extra {
		for _, export := range module.Exports {
			modules[module.Name] = append(modules[module.Name], m.export(module.Name, export))
		}
	}
	result := make([]Module, 0, len(modules))
	for name, exports := range modules {
		result = append(result, Module{Name: name, Exports: exports})
	}
	return result, nil
}

// contextModule describes the window's Node and locale. A template has no
// package identity, so it never receives a package or extension scope.
func (m *Mount) contextModule() Module {
	return Module{Name: "sailry", Exports: []Export{{Name: "context", Sync: func(context.Context, []json.RawMessage) (json.RawMessage, error) {
		body, err := json.Marshal(map[string]any{"api_version": "v1", "node": m.services.Node, "locale": m.locale})
		if err != nil {
			return nil, err
		}
		return json.Marshal(string(body))
	}}}}
}

// export bounds a product operation by the mount and reports a Node fault
// in the form scripts read with faultCode, as package views do.
func (m *Mount) export(module string, export Export) Export {
	wrap := func(operation Operation) Operation {
		if operation == nil {
			return nil
		}
		return func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
			if err := m.ctx.Err(); err != nil {
				return nil, scriptError(module, export.Name, err)
			}
			ctx, cancel := context.WithCancel(ctx)
			stop := context.AfterFunc(m.ctx, cancel)
			defer func() { stop(); cancel() }()
			value, err := operation(ctx, arguments)
			if err != nil {
				return nil, scriptError(module, export.Name, err)
			}
			return value, nil
		}
	}
	return Export{Name: export.Name, Sync: wrap(export.Sync), Async: wrap(export.Async)}
}

func scriptError(module, name string, err error) error {
	var fault *protocol.Fault
	if !errors.As(err, &fault) {
		var value protocol.Fault
		if errors.As(err, &value) {
			fault = &value
		}
	}
	if fault != nil {
		if body, marshalErr := json.Marshal(fault); marshalErr == nil {
			return fmt.Errorf("`%s.%s`: %s", module, name, body)
		}
	}
	return fmt.Errorf("`%s.%s`: %w", module, name, err)
}

type launchValue struct{ value any }

func (m *Mount) measureLaunch(c *ui.Context) {
	if m.services.TerminalLaunch != nil {
		m.launch.Store(launchValue{m.services.TerminalLaunch(c)})
	}
}

func (m *Mount) component(component Component) func(*ui.Context, plugins.NativeElement, func(), func(plugins.CallbackRef, json.RawMessage)) (ui.Element, error) {
	return func(c *ui.Context, element plugins.NativeElement, _ func(), _ func(plugins.CallbackRef, json.RawMessage)) (ui.Element, error) {
		props, err := componentProps(element.Props)
		if err != nil {
			return ui.Element{}, err
		}
		result, heading, err := component.Render(c, props, m.widgets.DrawingDetails())
		if heading != nil && err == nil {
			m.widgets.SetDetailsHeading(func(c *ui.Context) error { heading(c); return nil })
		}
		return result, err
	}
}

// componentProps reads a component's props in the forms the shared renderer
// accepts: positional [id] or [id, props], or one props object.
func componentProps(raw json.RawMessage) (map[string]json.RawMessage, error) {
	props := map[string]json.RawMessage{}
	if len(raw) == 0 || string(raw) == "null" {
		return props, nil
	}
	var positional []json.RawMessage
	if json.Unmarshal(raw, &positional) == nil {
		if len(positional) > 1 && string(positional[1]) != "null" && json.Unmarshal(positional[1], &props) != nil {
			return nil, errors.New("template component props must be an object")
		}
		return props, nil
	}
	if json.Unmarshal(raw, &props) != nil {
		return nil, errors.New("template component props must be an object")
	}
	return props, nil
}

func (m *Mount) frame(c *ui.Context, draw func(*ui.Context) error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.closed || m.err != nil || m.ctx.Err() != nil {
		return
	}
	m.active = c
	defer func() { m.active = nil }()
	if err := draw(c.Key(m)); err != nil {
		m.err = err
		kit.RecordFailure("desktop", "native_workspace", kit.Failed, err)
		m.services.Wake()
	}
}

func (m *Mount) Render(c *ui.Context) {
	m.frame(c, func(c *ui.Context) error {
		m.measureLaunch(c)
		_, err := m.view.Render(c)
		return err
	})
}

func (m *Mount) Locale() string { return m.locale }

// Failed reports whether the template stopped after a script or render failure.
func (m *Mount) Failed() bool {
	m.mu.Lock()
	defer m.mu.Unlock()
	return m.err != nil
}

func (m *Mount) Close() {
	m.mu.Lock()
	defer m.mu.Unlock()
	if !m.closed {
		m.closed = true
		m.cancel()
		if m.engine != nil {
			_ = m.engine.Close()
		}
		_ = m.host.Close()
		if m.release != nil {
			m.release()
		}
	}
}

func (m *Mount) Slots() shell.WorkspaceSlots {
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.closed || m.err != nil {
		return shell.WorkspaceSlots{}
	}
	layout := m.widgets.WorkspaceLayout()
	wrap := func(draw func(*ui.Context) error) func(*ui.Context) {
		if draw == nil {
			return nil
		}
		return func(c *ui.Context) { m.frame(c, draw) }
	}
	slots := shell.WorkspaceSlots{
		PreferenceKey: m.preferences, MinNavigationWidth: layout.MinNavigationWidth,
		MinDetailsWidth: layout.MinDetailsWidth, DefaultDetailsWidth: layout.DefaultDetailsWidth, DetailsLabel: layout.DetailsLabel,
		Navigation: wrap(m.left), Details: wrap(m.right), DetailsHeader: wrap(m.widgets.DetailsHeading()),
		Title: wrap(m.renderTitle), Header: wrap(m.renderHeader),
	}
	if m.closeTab != "" {
		slots.Close = func(*ui.Context) { m.widgets.EmitHeaderAction(m.closeTab) }
	}
	return slots
}

type header struct {
	Title string `json:"title"`
	Tabs  *struct {
		ID         string            `json:"id"`
		Items      []json.RawMessage `json:"items"`
		Selected   *string           `json:"selected"`
		MaxWidth   *float32          `json:"max_width"`
		CloseLabel string            `json:"close_label"`
	} `json:"tabs"`
	Actions []struct {
		ID       string `json:"id"`
		Label    string `json:"label"`
		Icon     string `json:"icon"`
		Disabled bool   `json:"disabled"`
	} `json:"actions"`
}

func (m *Mount) renderTitle(c *ui.Context) error {
	if tabs := m.title.Tabs; tabs != nil && len(tabs.Items) != 0 && m.tabs.Render != nil {
		props, err := json.Marshal([]any{tabs.ID, tabs})
		if err != nil {
			return err
		}
		_, err = m.tabs.Render(c.Key("title-tabs"), plugins.NativeElement{Module: m.tabs.Module, Export: m.tabs.Export, Props: props}, func() {}, func(plugins.CallbackRef, json.RawMessage) {})
		return err
	}
	ui.Text(c.Key("title"), m.title.Title).FontSize(14).SingleLine().Shrink(1).TextColor(theme.Of(c).Foreground)
	return nil
}

func (m *Mount) renderHeader(c *ui.Context) error {
	if len(m.title.Actions) == 0 {
		return nil
	}
	ui.Row(c.Key("header")).MinWidth(0).Shrink(1).Gap(12).AlignItems(ui.Center).Children(func() {
		ui.Row(c.Key("actions")).Shrink(0).Gap(shell.ButtonGap).AlignItems(ui.Center).Children(func() {
			for _, action := range m.title.Actions {
				icon, _ := icons.Plugin(action.Icon)
				kit.IconButton(c.Key(action.ID), icon, action.Label, shell.HeaderButton, false).Disabled(action.Disabled).
					OnClick(func() { m.widgets.EmitHeaderAction(action.ID) })
			}
		})
	})
	return nil
}

func (m *Mount) toast(_ context.Context, args []json.RawMessage) (json.RawMessage, error) {
	var value struct{ ID, Kind, Message string }
	if len(args) != 1 || json.Unmarshal(args[0], &value) != nil || value.ID == "" || value.Message == "" ||
		value.Kind != "error" && value.Kind != "info" {
		return nil, errors.New("invalid template notification")
	}
	if !m.services.Post(func(c *ui.Context) {
		if m.ctx.Err() == nil {
			c.AddToast(ui.Toast{ID: fmt.Sprintf("native-template/%p/%s", m, value.ID), Title: value.Message, Type: value.Kind, Timeout: kit.ToastDuration(c)})
		}
	}) {
		return nil, errors.New("template is closed")
	}
	return json.RawMessage("null"), nil
}
