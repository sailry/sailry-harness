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

// ProviderDraft captures one editor revision; secrets never return to the form.
type ProviderDraft struct {
	Channel          viewmodel.Channel
	Key              string
	ExpectedRevision uint64
	OAuth            [4]string
}
type ProviderActions struct {
	NewID          func() string
	Available      func() bool
	Save           func(ProviderDraft, func(bool))
	Discover       func(ProviderDraft, func([]viewmodel.ChannelModel))
	Enable         func(string, uint64, bool)
	Remove         func(string, uint64)
	ReadKey        func(string, func(bool))
	RefreshCatalog func()
	SaveCompaction func()
	SignIn         func(string)
	SubmitLogin    func(string)
	CancelLogin    func()
	// Usage is what the provider slot holds for the channel, nil where no
	// enabled package publishes a figure for it.
	Usage func(c *ui.Context, channel string) *ChannelUsage
}
