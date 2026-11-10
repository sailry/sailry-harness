package settings

import (
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// The settings' view model: the boundary between the pages and their
// logic (docs/settings.md). Pages read and edit these fields; the logic
// half loads them, validates and saves them, and reports how a save went.
//
// Two kinds of settings, as in the reference: - App preferences are the app's
// own and apply as they change; they are plain fields. - Node settings
// (power, telemetry, sandbox, the terminal host, hooks, the Python index,
// local models, backup) are a host's, saved by a revisioned request: Stored
// keeps the saved value, the draft the page edits and the revision the draft
// is based on.

// Stored is a Node setting: Saved at Revision, the Draft the page edits,
// and Pending while a save is under way, which locks the page's controls.
type Stored[T any] struct {
	Saved, Draft T
	Revision     uint64
	Pending      bool
}

// Stored creates a setting saved as v.
func stored[T any](v T) Stored[T] { return Stored[T]{Saved: v, Draft: v, Revision: 1} }

// Commit saves the draft. The preview has no Node: the draft becomes the
// saved value in memory and the save is Done. The logic half sends the
// request with Revision and returns its outcome: Failed keeps the draft,
// Conflict keeps it until the saved value is read again, Uncertain keeps
// it while the result is unknown.
func (s *Stored[T]) Commit() kit.Outcome {
	s.Saved = s.Draft
	s.Revision++
	return kit.Done
}

// Messages are the toasts of a save's outcomes, by i18n key; "" shows
// none, as the reference's switches show no success toast.
type Messages struct{ Done, Failed, Conflict, Uncertain string }

// report shows a save's outcome as a toast, the only place outcomes show.
func report(c *ui.Context, o kit.Outcome, m Messages) {
	key := map[kit.Outcome]string{kit.Done: m.Done, kit.Failed: m.Failed, kit.Conflict: m.Conflict, kit.Uncertain: m.Uncertain}[o]
	if key != "" {
		kit.Toast(c, o, i18n.T(c, key))
	}
}

// Report delivers an asynchronous settings save result through the same toasts.
func Report(c *ui.Context, o kit.Outcome, m Messages) { report(c, o, m) }

// notice is the toast of an action the preview does not carry out: one
// that would read the disk, use the network, pair or run a model.
func notice(c *ui.Context) { kit.Message(c, i18n.T(c, "preview_notice")) }

// Telemetry is what diagnostics send.
type Telemetry struct{ Errors, Metrics bool }

// TerminalHost is a host's terminal settings: its environment and the shell,
// an index of Shells.
type TerminalHost struct {
	Env   []preview.EnvVar
	Shell int
}

// HookSettings are a project's hooks: on or off, and their scripts.
type HookSettings struct {
	Enabled bool
	Scripts []preview.Hook
}

// Python is the host's package index: an index of Indexes, the last being a
// custom URL.
type Python struct {
	Index  int
	Custom string
}

// LocalModels is a kind of local model's page: enabled, the model chosen
// among the installed ones, the Installed or Available tab, and the models.
type LocalModels struct {
	Enabled Stored[bool]
	Model   int // index of the installed models
	Tab     int // 0 installed, 1 available
	Query   string
	Models  []preview.LocalModel
	Catalog []preview.LocalModel
	// A live page's: its kind (a Models* key), the chosen model's name when
	// the host no longer has it installed (the picker's last choice), and
	// whether the host cannot run this kind.
	Kind        string
	Missing     string
	Unavailable bool
}

// Installed are the models a host has.
func (m *LocalModels) Installed() []preview.LocalModel {
	var out []preview.LocalModel
	for _, x := range m.Models {
		if x.Status == preview.ModelInstalled {
			out = append(out, x)
		}
	}
	return out
}

// BackupTarget is where backups go: WebDAV, S3 or a local folder (Tab), its
// fields, and the schedule. Secrets are never loaded: Identity, Secret, Token
// and the passphrases are what the user types to replace them, the saved ones
// shown as masked placeholders.
type BackupTarget struct {
	Tab                              int // 0 WebDAV, 1 S3, 2 local
	Endpoint, Prefix, Bucket, Region string
	Identity, Secret, Token          string
	Passphrase, Confirm              string
	Directory                        string
	Schedule                         bool
	Time                             time.Time // the daily time
	Timezone                         string
	PassphraseSet, CredentialsSet    bool
}

// Model is the settings pages' view model.
type Model struct {
	// Live keeps later-phase controls unavailable and routes saves to the Client.
	Live              bool
	SavePower         func()
	SaveSandbox       func()
	SaveHooks         func()
	HooksContext      func() (string, bool)
	HooksSandboxed    func() bool
	TeamActions       *TeamActions
	SaveTelemetry     func()
	SaveTerminal      func()
	ProviderActions   ProviderActions
	ConnectorActions  *ConnectorActions
	PackageActions    *PackageActions
	AppearanceActions *AppearanceActions
	ModelActions      *ModelActions
	// General.
	Language       int // of Languages
	SidebarMetrics bool
	Notify         [4]bool // system, approval, completed, failed
	Toast          int     // of ToastSeconds
	Power          Stored[bool]
	Telemetry      Stored[Telemetry]

	// Conversation.
	Sandbox        Stored[bool]
	SessionOrder   int     // 0 created, 1 updated
	MessageDisplay int     // 0 detailed, 1 compact
	TextSize       float64 // 12..18 by 2

	// Shortcuts.
	Keys Keys

	// Dictation.
	MicrophoneGranted bool
	Microphone        int // 0 default, then Microphones
	Microphones       []string
	SpeechLanguage    int // 0 auto, 1 Chinese, 2 English
	SpeechModel       int // of Speech
	SpeechTab         int // 0 installed, 1 available
	SpeechQuery       string
	Speech            []preview.LocalModel
	info              *modelInfo // the model whose details show

	// Providers (settings/providers): the channels, the directory's
	// brands, the catalog, the compaction threshold and the family
	// filter ("" for all).
	Channels   []preview.Channel
	Brands     []preview.Brand
	Catalog    preview.Catalog
	Compaction Stored[float64]
	Family     string
	expanded   string // the channel whose models show
	provider   providerEditor
	login      Authorization
	loginFor   string
	removing   string

	// Connectors and MCP: the services, the connector types plugins declare, the
	// host's CLI accounts, the search and the method filter (of
	// connectorMethods).
	Services     []preview.Service
	ConnTypes    []preview.ConnectorType
	ConnAccounts []string
	ConnQuery    string
	ConnMethod   int
	connector    connectorEditor
	mcp          mcpEditor
	connDetails  string
	connRemoving string

	// Teams: the teams, the roles, the child concurrency and the tab (0 teams, 1
	// roles).
	Teams        []preview.Team
	Roles        []preview.Role
	Concurrency  Stored[float64]
	TeamTab      int
	teamEdit     teamEditor
	roleEdit     roleEditor
	teamRemoving string
	roleRemoving string

	// Plugins, Skills, Market and the model library (plugins*, skills,
	// market*, model_library*): the installed packages, the market's
	// plugins, skills and themes, the installed skills and the library's
	// models, with the pages' searches and filters.
	Packages        []preview.Package
	MarketPlugins   []preview.Package
	SkillList       []preview.Skill
	MarketSkills    []preview.Skill
	Themes          []preview.MarketTheme
	Library         []preview.LibraryModel
	SkillQuery      string
	MarketKind      int // 0 plugins, 1 skills, 2 themes
	MarketQuery     string
	MarketSource    int // 0 official, 1 community
	MarketCategory  int // of marketCategories
	MarketMode      int // 0 all, 1 dark, 2 light
	MarketInstalled bool
	// MarketLoad is the catalog's load: the cards' skeleton shows while
	// it runs, from the market's first frame.
	MarketLoad preview.Fetch
	// LibraryLoad is the model library's search: placeholder rows before
	// its first page, a spinner over the results after.
	LibraryLoad   preview.Fetch
	config        pluginConfig
	install       pluginInstall
	download      pluginDownload
	updates       pluginUpdates
	checked       bool // the plugins' update check ran: its updates show in the header
	uninstalling  string
	skillAdd      skillInstall
	skillRemoving string
	library       library

	// Terminal.
	Host            Stored[TerminalHost]
	Shells          []preview.Shell
	PasteProtection bool
	FontSize        float64 // 8..32
	Font            string
	env             envEditor

	// Connections.
	Devices              []viewmodel.Device
	Relays               []string
	ConnectionRelays     []RelayChoice
	ConnectionActions    *ConnectionActions
	ConnectionPending    bool
	ConnectionStage      string
	Share                ShareState
	NetworkEditor        *NetworkEditor
	HostTrust            *HostTrust
	pairing              pairing
	connectionGeneration uint64

	// Hooks (hooks/*).
	Hooks   Stored[HookSettings]
	hook    hookEditor
	consent consent

	// Runtime and OCR (runtime/*, models/*).
	PythonVersion string
	Indexes       []string
	Python        Stored[Python]
	OCR           LocalModels
	Knowledge     LocalModels // embedding models (knowledge-settings)
	Decision      LocalModels // decision models

	// Permissions.
	Permissions []preview.Permission

	// Backup (backup/*).
	Backup   Stored[BackupTarget]
	Archived []preview.Archived
	Sessions int
	backups  backupDialogs

	// About.
	Version string
	Channel int // 0 stable, 1 experience
	// Updater is the app's updater (About).
	Updater Updater
}

// Languages are the language choices.
var Languages = []string{
	"settings_language_system", "settings_language_english", "settings_language_chinese",
	"settings_language_traditional_chinese", "settings_language_japanese", "settings_language_korean",
	"settings_language_french", "settings_language_german", "settings_language_spanish",
	"settings_language_portuguese_brazil", "settings_language_russian",
}

// ToastSeconds are the toast durations offered.
var ToastSeconds = []int{3, 5, 8, 10}

// NewModel builds the view model over the settings' dataset.
func NewModel(d *preview.Settings) *Model {
	pr := preview.ProvidersData()
	cn := preview.ConnectorsData()
	td := preview.TeamsData()
	ex := preview.ExtensionsData()
	m := &Model{
		Channels: pr.Channels, Brands: pr.Brands, Catalog: pr.Catalog, Compaction: stored(pr.Compaction),
		Services: cn.Services, ConnTypes: cn.Types, ConnAccounts: cn.Accounts,
		Teams: td.Teams, Roles: td.Roles, Concurrency: stored(td.Concurrency),
		Packages: ex.Packages, MarketPlugins: ex.Catalog, SkillList: ex.Skills, MarketSkills: ex.Market, Themes: ex.Themes, Library: ex.Library,
		Notify: [4]bool{true, true, true, true}, Toast: 1,
		Power:     stored(d.PreventSleep),
		Telemetry: stored(Telemetry{Errors: d.Errors, Metrics: d.Metrics}),
		Sandbox:   stored(d.Sandbox), TextSize: 14,
		Keys:        NewKeys(),
		Microphones: d.Microphones, Speech: d.Speech,
		Host:   stored(TerminalHost{Env: d.Env}),
		Shells: d.Shells, PasteProtection: true, FontSize: 13,
		Devices: d.Devices, Relays: d.Relays,
		Hooks:         stored(HookSettings{Enabled: d.HooksOn, Scripts: d.Hooks}),
		PythonVersion: d.Python, Indexes: d.Indexes, Python: stored(Python{}),
		OCR:         LocalModels{Enabled: stored(true), Models: d.OCR, Catalog: d.OCRCatalog},
		Knowledge:   LocalModels{Enabled: stored(true), Models: d.Embedding, Catalog: d.Embedding},
		Decision:    LocalModels{Enabled: stored(false), Model: -1, Models: d.Decision, Catalog: d.Decision},
		Permissions: d.Permissions,
		Backup: stored(BackupTarget{Endpoint: "https://dav.example.com/remote.php/dav", Prefix: "sailry",
			Schedule: true, Time: time.Date(2026, 1, 1, 3, 0, 0, 0, time.Local), Timezone: "Asia/Shanghai", PassphraseSet: true, CredentialsSet: true}),
		Archived: d.Archived, Sessions: d.Sessions,
		Version: d.Version,
	}
	m.MarketLoad.Start()
	return m
}
