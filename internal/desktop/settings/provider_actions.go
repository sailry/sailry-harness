package settings

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// ChannelUsage is the account figure the packages enabled on the Node publish
// for a channel in the provider slot: the lines its row shows, the icon before
// a line that is a value alone, and Card, which shows every figure in a hover
// card over the element it is given.
type ChannelUsage struct {
	Lines []UsageLine
	Icon  *ui.SVG
	Card  func(c *ui.Context, over ui.Element)
}

// UsageLine is a line of a channel's figure: a value, and with Meter its
// label and a bar filled to Progress, 0 to 100.
type UsageLine struct {
	Label, Value string
	Meter        bool
	Progress     float64
}

// ProviderDraft captures one editor revision. Key holds a new or changed API
// key; an unchanged stored credential is retained by the Node.
type ProviderDraft struct {
	Channel          viewmodel.Channel
	Key              string
	ExpectedRevision uint64
	OAuth            [4]string
	Group            string
	Targets          []string
	TargetRevisions  map[string]uint64
}

// ProviderHost is a Node available to own a model channel.
type ProviderHost struct {
	Node, Name, Platform string
	Connected            bool
	// Local marks this computer's Node, which shows as the local device
	// rather than its network hostname.
	Local bool
}

// ProviderSaveResult reports each Node's outcome for a channel save.
type ProviderSaveResult struct {
	// Resume settles outstanding requests and applies edits to the same replicas.
	Resume          func(ProviderDraft, func(ProviderSaveResult))
	Succeeded       []string
	Failed          []string
	Uncertain       []string
	Unconfirmed     []string // The original outcome can no longer be resolved.
	Group           string
	TargetIDs       map[string]string
	TargetRevisions map[string]uint64
}

type ProviderActions struct {
	NewID          func() string
	Available      func() bool
	Hosts          func() []ProviderHost
	DefaultTarget  func() string
	CatalogAt      func(node string) viewmodel.Catalog
	Save           func(ProviderDraft, func(ProviderSaveResult))
	Discover       func(ProviderDraft, func([]viewmodel.ChannelModel))
	LookupModel    func(ProviderDraft, string, func(*viewmodel.ChannelModel))
	Enable         func(node, id string, revision uint64, enabled bool)
	Remove         func(node, id string, revision uint64)
	ReadKey        func(node, id string, done func(string))
	RefreshCatalog func()
	SaveCompaction func()
	SignIn         func(node, id string)
	// Usage is what the provider slot holds for the channel, nil where no
	// enabled package publishes a figure for it.
	Usage func(c *ui.Context, node, channel string) *ChannelUsage
}
