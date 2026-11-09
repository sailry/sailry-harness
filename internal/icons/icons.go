// Package icons holds the SVG icons the reference app draws: Lucide icons as
// Kit bundles them (svg/LICENSE-LUCIDE), Sailry's reicon glyphs and provider
// logos (svg/PROVIDERS-SOURCE.md). ui.Icon draws them in the text color.
package icons

import (
	"embed"

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

	Chat       = load("reicon-messages-chat-round")
	Grid       = load("reicon-ui-grid")
	Puzzle     = load("reicon-ui-puzzle-piece")
	Microphone = load("reicon-microphone")
	Writing    = load("reicon-writing")
	Help       = load("reicon-help-circle")

	Providers = map[string]*ui.SVG{
		"anthropic": load("provider-anthropic"),
		"openai":    load("provider-openai"),
		"google":    load("provider-gemini"),
		"deepseek":  load("provider-deepseek"),
	}
)
