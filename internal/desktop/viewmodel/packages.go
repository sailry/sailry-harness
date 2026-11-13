package viewmodel

type PackageKind int

const (
	KindPlugin PackageKind = iota
	KindSkill
	KindMCP
)

type Setting struct {
	Name, Title, Kind string
	Options           []string
	Value             string
	Set, Required     bool
	// Model names the model field an "effort" setting follows; Efforts are
	// the levels each of that field's models takes, by model ID.
	Model   string
	Efforts map[string][]string
}

// SettingsTab is a page of a package's settings: its fields, all of them
// shown only while the Enabled field, when named, is on.
type SettingsTab struct {
	ID, Title string
	Fields    []string
	Enabled   string
}

type Package struct {
	ID, Name, Description string
	Version, Latest       string
	Requires              string
	Kind                  PackageKind
	Category              string
	Source                string
	Host                  string
	InstallName           string
	Official, Enabled     bool
	Installed             bool
	Tools, Skills, MCP    []string
	Permissions           []string
	// AddedPermissions are the permissions the Latest version asks that
	// the installed one does not have.
	AddedPermissions []string
	Notes            string
	Settings         []Setting
	Versions         []string
	// Icon is the icon the package declares for itself: an icon name
	// ("phosphor:<name>", "ailogo:<slug>", "reicon:<group>/<name>") or its
	// own SVG markup. Tint is its declared tile color ("blue", "violet",
	// "teal", "amber", "gray").
	Icon, Tint string
}

type Skill struct {
	ID, Name, Description string
	Package               string
	Origin                string
	Enabled, Update       bool
	Installed             bool
	Content               string
}

type MarketTheme struct {
	ID, Name, Description       string
	Dark, Light                 bool
	Installed, Builtin, Current bool
	Update                      bool
}
