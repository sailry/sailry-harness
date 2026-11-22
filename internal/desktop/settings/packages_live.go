package settings

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

type SkillSourceDraft struct {
	Source                           int
	Repository, Revision, Path, Link string
}

type PackageActions struct {
	Catalog        *PackageCatalogService
	Selection      *SkillSelection
	CheckUpdates   func(viewmodel.PackageKind)
	Update         func([]string)
	Toggle         func(string, bool)
	Configure      func(string)
	Uninstall      func(string, func(bool))
	Install        func(string, string, func(bool))
	Browse         func(string, string, func() bool, func(string)) // a package file of this machine, where Install reads it: the local host's name, the directory first shown, whether the form still wants it, the chosen path
	Download       func(string, string, string, func(bool))
	CancelDownload func()
	// LoadDetails finishes with a failure's i18n key and the host's reason,
	// both "" when the details loaded.
	LoadDetails   func(DetailKind, string, func(failure, reason string))
	RetryMarket   func()
	MoreMarket    func()
	MarketPlugin  func(string)
	PluginVersion func(string, string)
	PinPlugin     func(string, bool)
	MarketSkill   func(string)
	Theme         func(string)
	ChooseSkill   func(int, func(string))
	InstallSkill  func(SkillSourceDraft, func(bool))
	CancelSkill   func()
	ReimportSkill func(string)
	SavePython    func()
	About         func()
	// Image is a market card's picture by its https address, nil until
	// fetched or when it cannot be.
	Image func(string) *ui.Bitmap
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

// OpenPackageConfiguration shows a package's settings, on tabs when the
// package declares them.
func (m *Model) OpenPackageConfiguration(id, name string, fields []viewmodel.Setting, tabs ...viewmodel.SettingsTab) {
	m.config = pluginConfig{open: true, generation: m.config.generation + 1, id: id, name: name, fields: fields, secrets: map[string]int{}, tabs: tabs}
}

// OpenNodePackageConfiguration binds the form and its models to its owner.
func (m *Model) OpenNodePackageConfiguration(id, name, node string, fields []viewmodel.Setting, tabs []viewmodel.SettingsTab, save func([]viewmodel.Setting, map[string]int, func(bool))) {
	m.OpenPackageConfiguration(id, name, fields, tabs...)
	m.config.node, m.config.save = node, save
}

func (m *Model) ClosePackageConfiguration() {
	m.config = pluginConfig{generation: m.config.generation + 1}
}

func (m *Model) CloseManagedPackageDialogs() {
	m.ClosePackageConfiguration()
	m.install = pluginInstall{generation: m.install.generation + 1}
	m.download = pluginDownload{generation: m.download.generation + 1}
	if page := m.SkillPageState; page != nil {
		page.CloseDialogs()
	}
	m.updates, m.uninstalling, m.checked = pluginUpdates{}, "", false
	if m.PackageActions != nil {
		if selection := m.PackageActions.Selection; selection != nil {
			selection.Open = false
			if !selection.Pending && selection.Cancel != nil {
				selection.Cancel()
				selection.Cancel = nil
			}
		}
	}
}

func (m *Model) MarkUpdatesChecked() { m.checked = true }

// The operations a button starts and waits for (Model.Busy).
const (
	BusyCatalog       = "catalog"     // Providers: Update
	BusyThemes        = "themes"      // Appearance: Check updates
	BusyPluginUpdates = "plugins"     // Plugins: Check updates
	BusySkillUpdates  = "skills"      // Skills: Check updates
	BusyMarket        = "market"      // Market: Load more, while the live search reads
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
