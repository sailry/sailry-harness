// Package viewapi defines the window-scoped service contracts shared between
// live Desktop windows and compiled view modules.
package viewapi

import (
	"context"
	"io"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
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
	// Tools is optional. Its callback runs on the UI thread while mounted.
	Tools *ConversationToolObserver
}

type Conversation struct {
	Render func(*ui.Context)
	Header func(*ui.Context, string)
	Close  func()
}

// NodeInfo describes an execution Node visible to this window.
type NodeInfo struct {
	ID             protocol.NodeID
	Label          string
	Connected      bool
	Local          bool
	ProjectSources []string
	Tools          []protocol.HostTool
	CanClone       bool
}

// UsageSource identifies a Node included in usage reads and overviews.
type UsageSource struct {
	ID    protocol.NodeID
	Label string
	Local bool
}

// UsageNodeView is the current observation state of one Node in a usage overview.
type UsageNodeView struct {
	Report     *protocol.UsageReport
	Connected  bool
	Refreshing bool
	Error      *protocol.Fault
}

// UsageNodeSource pairs a Node route with its observed usage view.
type UsageNodeSource struct {
	Node  protocol.NodeID
	Entry protocol.NodeID
	View  UsageNodeView
}

// UsageScopedGroup tags a usage breakdown group with the Node that reported it.
type UsageScopedGroup struct {
	Node  protocol.NodeID
	Entry protocol.NodeID
	Group protocol.UsageGroup
}

// UsageScopedRequest tags a usage request record with its entry Node.
type UsageScopedRequest struct {
	Request protocol.UsageRequest
	Entry   protocol.NodeID
}

// UsageScopedPage is one cursor-paginated slice of merged usage requests.
type UsageScopedPage struct {
	Items   []UsageScopedRequest
	HasMore bool
}

// UsageSummary aggregates totals, time buckets, breakdown groups and requests
// across the Nodes observed by a usage overview.
type UsageSummary struct {
	Totals   protocol.UsageMetrics
	Days     []protocol.UsagePoint
	Recent   []protocol.UsagePoint
	Groups   []UsageScopedGroup
	Requests UsageScopedPage
}

// UsageOverview is one consistent snapshot produced by WatchOverview.
type UsageOverview struct {
	Sources  []UsageNodeSource
	Summary  *UsageSummary
	Complete bool
	Error    *protocol.Fault
}

// UsageWatch owns a live or sample usage overview subscription.
type UsageWatch struct {
	Refresh func(context.Context) error
	Close   func()
}

// UsageInventory holds the filter options available on one Node.
type UsageInventory struct {
	Node      protocol.NodeID
	Label     string
	Projects  []protocol.Project
	Providers []protocol.ConversationProvider
}

// UsageSample serves in-memory Demo usage reports and filter inventory without
// contacting any Node. Revision advances whenever the demo ledger changes.
type UsageSample struct {
	Revision  uint64
	Report    func(protocol.NodeID, protocol.UsageQuery) (protocol.UsageReport, error)
	Inventory func() ([]protocol.Project, []protocol.ConversationProvider)
}

// UsageServices exposes multi-node and Demo usage operations to native modules.
type UsageServices struct {
	Sources func() []UsageSource
	// WatchOverview observes usage for target, or across all active Sources
	// when target is the zero NodeID. onUpdate runs off the UI thread. Call
	// Close to release the watch; closing the window also stops active watches.
	WatchOverview func(context.Context, protocol.NodeID, protocol.UsageQuery, func(UsageOverview, error)) (UsageWatch, error)
	ReadUsage     func(context.Context, protocol.NodeID, protocol.UsageQuery) (protocol.UsageReport, error)
	Inventory     func(protocol.NodeID) (UsageInventory, bool)
	Sample        func() (*UsageSample, bool)
}

// ProviderAccount identifies a connected provider's credential binding.
// CredentialKnown is false until its credential metadata has loaded.
type ProviderAccount struct {
	Authentication     protocol.Authentication
	ProviderRevision   uint64
	Credential         protocol.CredentialID
	CredentialKnown    bool
	CredentialRevision uint64
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

	// Multi-node discovery, execution, snapshots and window-wide notifications.
	Nodes func() []NodeInfo
	// NodeLabel resolves the canonical localized display name for any Node ID:
	// the local host name in the window's active locale, the peer's canonical
	// name when known or remembered, or a consistent short ID fallback.
	NodeLabel func(protocol.NodeID) string
	// ModelOptions returns the canonical kit.ModelSelect options for enabled
	// model channels on the given Node (zero NodeID selects the local Node),
	// with Mark, Brand, BrandName, Efforts, DefaultEffort, and Score populated.
	ModelOptions    func(protocol.NodeID) []kit.ModelOption
	PrepareOn       func(protocol.NodeID, protocol.Command) (protocol.Request, error)
	ExecuteOn       func(context.Context, protocol.NodeID, protocol.Request) (protocol.Output, error)
	SettleOn        func(context.Context, protocol.NodeID, protocol.Request) (protocol.Output, error)
	ResolveOn       func(context.Context, protocol.NodeID, protocol.Request) (Receipt, error)
	OutcomeOn       func(context.Context, protocol.NodeID, protocol.Request) (protocol.RequestOutcome, error)
	Snapshot        func(protocol.NodeID) (*protocol.Snapshot, bool)
	ProviderAccount func(protocol.NodeID, protocol.ProviderID) (ProviderAccount, bool)
	Credentials     func(protocol.NodeID) map[protocol.CredentialID]protocol.Credential
	OpenSession     func(protocol.NodeID, protocol.SessionID)
	RevealDetails   func()
	OnChange        func(func()) func()
	Usage           *UsageServices

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
