package settings

import "github.com/sailry/sailry-harness/internal/desktop/viewmodel"

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
}
