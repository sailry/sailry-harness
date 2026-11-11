package icons

import (
	"fmt"
	"strings"
	"sync"

	"github.com/egoist/mygo/ui"
	"github.com/yldm-tech/mygo-kubai-icons/all"
	"github.com/yldm-tech/mygo-kubai-icons/kubai"
)

// Kubai Icons are a package's emblem: layered marks in an accent, drawn by
// the library itself, that play a motion on hover. They have no one-color
// form, so the glyphs of text-colored slots (the rail, rows, menus) stay
// the embedded Reicon outlines (Plugin).

// kubaiIcons are the library's icons by their upstream id.
var kubaiIcons = sync.OnceValue(func() map[string]func() *kubai.Icon {
	byName := make(map[string]func() *kubai.Icon, len(all.Entries))
	for _, entry := range all.Entries {
		byName[entry.Name] = entry.Icon
	}
	return byName
})

// reiconEmblems names the Kubai icon of a Reicon glyph whose own name the
// library does not have.
var reiconEmblems = map[string]string{
	"building/buildings":    "building",
	"building/city":         "building-office",
	"business/chart":        "chart-line",
	"business/diagram-tree": "sitemap",
	"it/code-square":        "file-code",
	"it/hashtag":            "hash",
	"it/terminal-square":    "terminal",
	"it/window":             "app-window",
	"map/pointer":           "map-pin",
	"messages/chat-round":   "message-circle",
	"newicons/book-square":  "book-open",
	"newicons/cards":        "playing-cards",
	"newicons/edit":         "pencil",
	"newicons/grid2":        "grid",
	"newicons/hierarchy":    "sitemap",
	"newicons/hierarchy2":   "git-branch",
	"newicons/key-square8":  "key",
	"tools/window-pointer":  "mouse-pointer-click",
	"ui/alert-triangle":     "triangle-alert",
	"ui/box":                "cube",
	"ui/dark-light":         "toggle",
	"ui/magnifier":          "search",
	"ui/puzzle-piece":       "puzzle",
	"ui/tasks":              "list-checks",
	"ui/trash2":             "trash",
	"users/users2":          "users",
}

// Emblem is the Kubai icon a declared icon name shows as a package's
// emblem, nil when it has none: "kubai:<id>" names any icon of the library
// by its upstream id, and a "reicon:<group>/<name>" glyph has the emblem
// of the same subject, the library's icon called <name> unless
// reiconEmblems names another.
func Emblem(name string) *kubai.Icon {
	id, found := strings.CutPrefix(name, "kubai:")
	if !found {
		path, reicon := strings.CutPrefix(name, "reicon:")
		if !reicon {
			return nil
		}
		if id = reiconEmblems[path]; id == "" {
			id = path[strings.LastIndexByte(path, '/')+1:]
		}
	}
	if icon := kubaiIcons()[id]; icon != nil {
		return icon()
	}
	return nil
}

// kubaiSVG is a "kubai:<id>" icon at rest in the library's own colors, for
// what takes an SVG. ui.Icon shows only its outline in the text color.
func kubaiSVG(name string) (*ui.SVG, error) {
	icon := Emblem(name)
	if icon == nil {
		return nil, fmt.Errorf("plugin icon %q is unavailable", name)
	}
	return icon.SVG(kubai.Options{}, false), nil
}
