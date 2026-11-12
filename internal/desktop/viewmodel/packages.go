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
