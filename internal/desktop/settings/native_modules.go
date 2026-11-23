package settings

import (
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

// BrowserProfile is one local Chrome profile offered for cookie import.
type BrowserProfile struct {
	ID   string
	Name string
}

// BrowserSettings holds the local built-in browser data preferences and the
// selected Node's managed external Chromium browser setting.
type BrowserSettings struct {
	Supported         bool
	Enabled           bool
	Persistent        bool
	Importing         bool
	ImportingProfile  string
	Profiles          []BrowserProfile
	ExternalEnabled   Stored[bool]
	ExternalAvailable bool
	ExternalLoaded    bool
	ExternalStatus    string
}

// BrowserActions are the live Browser settings page callbacks.
type BrowserActions struct {
	SetPersistent func(bool)
	ScanChrome    func()
	ImportChrome  func(profileID string)
	CloseProfiles func()
	SaveExternal  func()
}

// SearchDriver describes one web search service row on the selected Node.
type SearchDriver struct {
	ID              string
	Enabled         bool
	KeyConfigured   bool
	KeyRequired     bool
	BasicConfigured bool
	Endpoint        string
	Quota           string
	ModelProvider   string
	ModelID         string
}

// SearchCredentialUpdate describes how an API key or Basic Auth credential
// should be updated during a driver edit.
type SearchCredentialUpdate struct {
	Action   string // "keep", "set", "clear"
	Value    string
	Username string
	Password string
}

// SearchDriverDraft holds the edits submitted from one search driver dialog.
type SearchDriverDraft struct {
	Driver        string
	Endpoint      string
	Key           SearchCredentialUpdate
	Basic         SearchCredentialUpdate
	ModelProvider string
	ModelID       string
}

// SearchSettingsState holds the selected Node's search settings and model list.
type SearchSettingsState struct {
	Revision      uint64
	Loaded        bool
	Loading       bool
	Pending       bool
	Connected     bool
	Status        string
	Retryable     bool
	DefaultDriver string // "auto", "none", or driver ID
	Fallback      bool
	Drivers       []SearchDriver
	ModelOptions  []kit.ModelOption
}

// SearchActions are the live Web Search settings page callbacks.
type SearchActions struct {
	Reload        func()
	Retry         func()
	SetDefault    func(driver string)
	SetFallback   func(enabled bool)
	SetEnabled    func(driver string, enabled bool)
	Reorder       func(order []string)
	Test          func(driver string)
	SaveDriver    func(draft SearchDriverDraft, done func(ok bool, retry bool))
	ForgetPending func()
}

// MemoryConfig holds the selected Node's personal memory preferences.
type MemoryConfig struct {
	Enabled         bool
	AutoWrite       bool
	ContextBytes    uint32
	ReviewAfterDays uint32
}

// MemoryEntryItem is one memory summary row shown in Settings -> Memory.
type MemoryEntryItem struct {
	ID          string
	Project     string // "" for global
	Title       string
	Kind        string // "user", "feedback", "project", "reference"
	Archived    bool
	Revision    uint64
	UpdatedAtMs int64
	DuplicateOf string
	NeedsReview bool
}

// MemoryProjectOption is one project filter choice for Settings -> Memory.
type MemoryProjectOption struct {
	ID   string
	Name string
}

// MemoryMergeSource identifies an active memory archived during merge.
type MemoryMergeSource struct {
	ID       string
	Revision uint64
}

// MemorySaveDraft holds the payload for creating, editing, archiving or merging
// a memory entry.
type MemorySaveDraft struct {
	ID               string
	ExpectedRevision uint64
	Project          string
	Title            string
	Kind             string
	Body             string
	Archived         bool
	MergeSources     []MemoryMergeSource
}

// MemorySettingsState holds the selected Node's memory configuration and catalog.
type MemorySettingsState struct {
	Config      Stored[MemoryConfig]
	ConfigError string
	ConfigRetry bool
	Loaded      bool
	Loading     bool
	Connected   bool
	Status      string
	Error       string
	View        string // "active", "archived", "review"
	Scope       string // "all", "global", or project ID
	Query       string
	Projects    []MemoryProjectOption
	Entries     []MemoryEntryItem
	AllActive   []MemoryEntryItem
}

// MemoryActions are the live Memory settings page callbacks.
type MemoryActions struct {
	Refresh       func()
	SaveConfig    func()
	ReloadConfig  func()
	LoadEntry     func(id string, done func(entry MemoryEntryItem, body string, errKey string))
	SaveEntry     func(draft MemorySaveDraft, done func(ok bool, retry bool, errKey string))
	DeleteEntry   func(id string, revision uint64, done func(ok bool, retry bool, errKey string))
	ForgetPending func()
}

// ComputerSettingsState holds the selected Node's Computer Use toggle and
// execution-host system permissions.
type ComputerSettingsState struct {
	Enabled      Stored[bool]
	Loaded       bool
	Available    bool
	HelperError  string
	Status       string
	Checking     bool
	HostReadOnly bool
	Permissions  []preview.Permission
}

// ComputerActions are the live Computer Use settings page callbacks.
type ComputerActions struct {
	Save              func()
	CheckPermissions  func()
	RequestPermission func(resource string)
	OpenSettings      func(resource string)
	ShowApp           func()
}

// NativeModuleDialogHost lets native settings modules close their open modals
// when the selected Node changes.
type NativeModuleDialogHost interface {
	CloseDialogs()
}
