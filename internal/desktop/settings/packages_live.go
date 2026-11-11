package settings

import (
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

type SkillSourceDraft struct {
	Source                           int
	Repository, Revision, Path, Link string
}

type PackageActions struct {
	Selection      *SkillSelection
	CheckUpdates   func(viewmodel.PackageKind)
	Update         func([]string)
	Toggle         func(string, bool)
	Configure      func(string)
	SaveSettings   func(string, []viewmodel.Setting, map[string]int, func(bool))
	Uninstall      func(string, func(bool))
	Install        func(string, string, func(bool))
	Download       func(string, string, string, func(bool))
	CancelDownload func()
	LoadDetails    func(DetailKind, string, func())
	RetryMarket    func()
	MarketPlugin   func(string)
	MarketSkill    func(string)
	Theme          func(string)
	ChooseSkill    func(int, func(string))
	InstallSkill   func(SkillSourceDraft, func(bool))
	CancelSkill    func()
	ReimportSkill  func(string)
	SavePython     func()
	About          func()
}

type SkillCandidate struct {
	Name, Description string
	Selected          bool
}

type SkillSelection struct {
	Open, Pending bool
	Candidates    []SkillCandidate
	Install       func([]int, func(bool))
	Cancel        func()
}

type AppearanceActions struct {
	Installed    func(string) bool
	CheckUpdates func()
	Import       func()
	Export       func(*theme.Package)
	Uninstall    func(*theme.Package, func(bool))
	Preview      *ThemeImport
}

type ThemeImport struct {
	Package       *theme.Package
	Open, Pending bool
	Install       func(func(bool))
	details       *themeDetails
}

func (m *Model) OpenPackageConfiguration(id string, fields []viewmodel.Setting) {
	m.config = pluginConfig{open: true, generation: m.config.generation + 1, id: id, fields: fields, secrets: map[string]int{}}
}

func (m *Model) ClosePackageConfiguration() { m.config.open = false }

func (m *Model) MarkUpdatesChecked() { m.checked = true }

// The operations a button starts and waits for (Model.Busy).
const (
	BusyCatalog       = "catalog"     // Providers: Update
	BusyThemes        = "themes"      // Appearance: Check updates
	BusyPluginUpdates = "plugins"     // Plugins: Check updates
	BusySkillUpdates  = "skills"      // Skills: Check updates
	BusyPermissions   = "permissions" // Permissions: Check
	BusyTelemetry     = "telemetry"   // General: Reset
	// BusyPackage followed by a package's, skill's or theme's ID is an
	// install, update, switch or removal of it under way.
	BusyPackage = "package/"
	// BusyBackup followed by a backup action's name (export, now, archive,
	// delete, and unarchive/ or delete/ with a session's ID) is that action
	// under way.
	BusyBackup      = "backup/"
	BusyDiagnostics = "diagnostics" // General: Send
)

// SetBusy marks the operation op in flight, or over.
func (m *Model) SetBusy(op string, on bool) {
	if !on {
		delete(m.Busy, op)
		return
	}
	if m.Busy == nil {
		m.Busy = map[string]bool{}
	}
	m.Busy[op] = true
}

func (m *Model) PackageDraftOpen() bool { return m.install.open || m.download.open || m.skillAdd.open }

func (m *Model) OpenPackageUpdates() {
	m.updates = pluginUpdates{open: true, chosen: map[string]bool{}}
}
