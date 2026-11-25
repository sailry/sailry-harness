// Package view binds product-owned templates to the shared Desktop renderer.
package view

import (
	"context"
	"io"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type (
	Session       = protocol.Session
	SessionConfig = protocol.SessionConfig
	TerminalInfo  = protocol.TerminalInfo
)

type Change struct {
	Cursor    string `json:"cursor"`
	Connected bool   `json:"connected"`
}

type Receipt struct {
	State  string
	Output protocol.Output
	Fault  *protocol.Fault
}

type SourceFile struct {
	Reader   io.ReadCloser
	Size     uint64
	Revision string
}

type Destination struct {
	Writer  io.WriteCloser
	Publish func(context.Context) error
	Close   func()
}

type Terminal struct {
	Render func(*ui.Context)
	Focus  func()
	Close  func()
}

type ConversationSpec struct {
	Match func(Session) bool
	// Config supplies a draft configuration. Its resource, if any, is
	// captured at mount time by calling Config with a default configuration.
	Config func(SessionConfig) SessionConfig
}

type Conversation struct {
	Render func(*ui.Context)
	Header func(*ui.Context, string)
	Close  func()
}

// Services belong to one window and its local Node. They are for compiled
// contributions, which expose only their scoped operations to a template.
// Requests and local file paths must come from those native operations, not
// directly from a template. Persistent operations outlive a template.
type Services struct {
	Node      protocol.NodeID
	Connected func() bool
	Next      func(context.Context, string) (Change, error)
	Post      func(func(*ui.Context)) bool
	Wake      func()
	Async     func(func()) bool

	Prepare func(protocol.Command) protocol.Request
	Execute func(context.Context, protocol.Request) (protocol.Output, error)
	Settle  func(context.Context, protocol.Request) (protocol.Output, error)
	Resolve func(context.Context, protocol.Request) (Receipt, error)
	Outcome func(context.Context, protocol.Request) (protocol.RequestOutcome, error)

	// Mounting and interacting with these elements run on the window's UI thread.
	TerminalLaunch    func(*ui.Context) any
	MountTerminal     func(protocol.TerminalID, func(TerminalInfo) bool) Terminal
	MountConversation func(ConversationSpec) Conversation

	// Native pickers wait for a choice; call them outside the UI thread.
	PickFiles       func(context.Context) ([]string, error)
	PickFolder      func(context.Context) (string, error)
	PickDestination func(context.Context, string) (string, error)
	PickSecret      func(context.Context, string) ([]byte, string, error)
	WriteClipboard  func(string) error
	Preview         func(string, func(context.Context, uint64, func(uint64, uint64)) ([]byte, error)) error
	// File paths are captured by a native picker or drop, not by the template.
	OpenSource      func(context.Context, string, uint64) (SourceFile, error)
	OpenDestination func(string) (Destination, error)
	Upload          func(context.Context, protocol.StreamID, uint64, string, io.ReadCloser, func(uint64)) error
	Download        func(context.Context, protocol.StreamID, uint64, string, io.WriteCloser, func(uint64)) error
	ReleaseStream   func(protocol.StreamID)
}

type serviceKey struct{}

func WithServices(ctx context.Context, services Services) context.Context {
	return context.WithValue(ctx, serviceKey{}, services)
}

func WindowServices(ctx context.Context) (Services, bool) {
	if ctx == nil || ctx.Err() != nil {
		return Services{}, false
	}
	services, ok := ctx.Value(serviceKey{}).(Services)
	return services, ok
}
