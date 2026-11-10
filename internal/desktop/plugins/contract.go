package plugins

import (
	"context"
	"encoding/json"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type HostOperation func(context.Context, []json.RawMessage) (json.RawMessage, error)

type HostExport struct {
	Name  string
	Sync  HostOperation
	Async HostOperation
}

type HostModule struct {
	Name    string
	Exports []HostExport
}

type CallbackRef struct {
	Generation uint64 `json:"generation"`
	ID         uint64 `json:"id"`
}

type ElementStyle struct {
	Name      string            `json:"name"`
	Arguments []json.RawMessage `json:"arguments"`
}

type NativeElement struct {
	ID          uint64
	Module      string
	Export      string
	Text        string
	Props       json.RawMessage
	Styles      []ElementStyle
	Children    []*NativeElement
	Slots       map[string][]*NativeElement
	Callbacks   map[string]CallbackRef
	RenderChild func(*ui.Context, *NativeElement) (ui.Element, error) `json:"-"`
	Focus       func() bool                                           `json:"-"`
}

type NativeComponent struct {
	Module string
	Export string
	Render func(*ui.Context, NativeElement, func(), func(CallbackRef, json.RawMessage)) (ui.Element, error)
}

type ContributionHooks struct {
	Publish func(json.RawMessage) error
	Close   func(string) error
	Next    func(context.Context) (json.RawMessage, error)
}

type RuntimeBindings struct {
	Modules       []HostModule
	Components    []NativeComponent
	Contributions ContributionHooks
	Invalidate    func()
	Post          func(func())
	BindActions   func(*ui.Context, ui.Element, NativeElement, func(CallbackRef, json.RawMessage))
}

type Bundle = protocol.PluginDesktopBundle
type PluginContext = protocol.PluginContext
