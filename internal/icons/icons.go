// Package icons holds the SVG icons the reference app draws: Lucide icons as
// Kit bundles them (svg/LICENSE-LUCIDE), Sailry's reicon glyphs, the Remix
// Icon status glyphs of assets/icons/remix (Apache-2.0), the Simple Icons
// platform logos of assets/icons/os (CC0, svg/LICENSE-OS) and provider logos
// (svg/PROVIDERS-SOURCE.md). ui.Icon draws them in the text color.
package icons

import (
	"embed"
	"sync"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

//go:embed svg/*.svg
var files embed.FS

func load(name string) *ui.SVG {
	data, err := files.ReadFile("svg/" + name + ".svg")
	if err != nil {
		panic(err)
	}
	return ui.MustParseSVG(data)
}

var (
	Plus             = load("plus")
	Search           = load("search")
	Settings         = load("settings")
	Settings2        = load("settings-2")
	Ellipsis         = load("ellipsis")
	EllipsisVertical = load("ellipsis-vertical")
	ChevronDown      = load("chevron-down")
	ChevronRight     = load("chevron-right")
	PanelLeftClose   = load("panel-left-close")
	PanelLeftOpen    = load("panel-left-open")
	PanelRight       = load("panel-right")
	Network          = load("network")
	Folder           = load("folder")
	Info             = load("info")
	Inbox            = load("inbox")
	Cpu              = load("cpu")
	Palette          = load("palette")
	BookOpen         = load("book-open")
	ALargeSmall      = load("a-large-small")
	Gallery          = load("gallery-vertical-end")
	Bot              = load("bot")
	ArrowUp          = load("arrow-up")
	ArrowDown        = load("arrow-down")
	Stop             = load("window-maximize") // Kit's fallback stop glyph
	Check            = load("check")
	CircleCheck      = load("circle-check")
	TriangleAlert    = load("triangle-alert")
	CircleX          = load("circle-x")
	Loader           = load("loader-circle")
	Copy             = load("copy")
	RotateCw         = load("rotate-cw")
	Volume           = load("volume-2")
	Brain            = load("brain")
	Pin              = load("pin")
	GitBranch        = load("git-branch")
	Plug             = load("plug")
	Users            = load("users")
	BrainCircuit     = load("brain-circuit")
	Upload           = load("upload")
	Close            = load("x")
	Terminal         = load("square-terminal")
	Sun              = load("sun")
	Moon             = load("moon")
	Monitor          = load("monitor")

	FileText   = load("file-text")
	Minus      = load("minus")
	Pause      = load("pause")
	ArrowRight = load("arrow-right")
	CircleUser = load("circle-user")
	Eye        = load("eye")
	Dashboard  = load("layout-dashboard")

	Bell         = load("bell")
	CheckCheck   = load("check-check")
	Trash        = load("trash")
	PinOff       = load("pin-off")
	Undo         = load("undo-2")
	Redo         = load("redo")
	ArrowLeft    = load("arrow-left")
	ChevronLeft  = load("chevron-left")
	FolderOpen   = load("folder-open")
	ExternalLink = load("external-link")
	ShieldAlert  = load("shield-alert")
	Question     = load("message-circle-question-mark")
	File         = load("file")
	Web          = load("globe")
	Pencil       = load("pencil")
	Archive      = load("archive")
	Reload       = load("refresh-cw")
	Save         = load("save")
	Code         = load("code-xml")
	Restore      = load("rotate-ccw")

	Chat       = load("reicon-messages-chat-round")
	Command    = load("reicon-it-terminal-square")
	Globe      = load("reicon-map-globe")
	Tasks      = load("reicon-ui-tasks")
	Grid       = load("reicon-ui-grid")
	Puzzle     = load("reicon-ui-puzzle-piece")
	Microphone = load("reicon-microphone")
	Writing    = load("reicon-writing")
	Help       = load("reicon-help-circle")

	// Activity lanes (activity/icons.rs).
	ProgressWaiting   = load("remix-progress-4")
	ProgressRunning   = load("remix-progress-2")
	ProgressCompleted = load("remix-progress-6")
	// Phase 4a: transcript blocks, file preview, image viewer, usage.
	ListChecks     = load("list-checks")
	Square         = load("square")
	Minimize       = load("minimize")
	ChartPie       = load("chart-pie")
	Gauge          = load("gauge")
	DollarSign     = load("dollar-sign")
	MemoryStick    = load("memory-stick")
	MessagesSquare = load("messages-square")
	Image          = load("image")
	ChevronUp      = load("chevron-up")
	Quote          = load("text-quote")
	FileCode       = load("file-code")
	FileImage      = load("file-image")

	// The composer's (queue, modes, model controls, file picker).
	Replace    = load("replace")
	Map        = load("map")
	Crown      = load("crown")
	Zap        = load("zap")
	House      = load("house")
	Download   = load("download")
	HardDrive  = load("hard-drive")
	MenuLines  = load("menu")
	Delete     = load("delete")
	Shield     = load("reicon-shield")
	ShieldOff  = load("reicon-shield-off")
	Device     = load("reicon-device")
	Smartphone = load("reicon-smartphone")
	Android    = load("os-android")
	Apple      = load("os-apple")
	Linux      = load("os-linux")
	Windows    = load("os-windows11")

	Providers = map[string]*ui.SVG{
		"anthropic": load("provider-anthropic"),
		"openai":    load("provider-openai"),
		"google":    load("provider-gemini"),
		"deepseek":  load("provider-deepseek"),
	}
)

// named are the icons a view model names by string, as tools declare theirs.
var named map[string]*ui.SVG

func init() {
	named = map[string]*ui.SVG{
		"book-open": BookOpen, "search": Search, "file-text": FileText, "bot": Bot, "settings-2": Settings2,
		"circle-check": CircleCheck, "command": Command, "globe": Globe, "tasks": Tasks, "writing": Writing,
		"terminal": Terminal, "network": Network, "folder": Folder, "triangle-alert": TriangleAlert, "circle-x": CircleX,
	}
}

// ByName returns a named icon, nil when unknown.
func ByName(name string) *ui.SVG { return named[name] }

// menuIcons are the menu item icons made so far, by their SVG.
var menuIcons sync.Map

// Menu returns icon as a menu item's template icon (ui.MenuItem.Icon),
// made once and kept.
func Menu(icon *ui.SVG) ui.MenuIcon {
	if v, ok := menuIcons.Load(icon); ok {
		return v.(ui.MenuIcon)
	}
	mi, err := mygo.NewMenuIconSVG(icon, true)
	if err != nil {
		panic(err)
	}
	v, _ := menuIcons.LoadOrStore(icon, mi)
	return v.(ui.MenuIcon)
}
