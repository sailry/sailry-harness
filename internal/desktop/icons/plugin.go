package icons

import (
	"fmt"
	"strings"
	"sync"

	"github.com/egoist/mygo/ui"
)

var pluginIcons sync.Map

var pluginAliases = map[string]string{
	"close":                            "x",
	"terminal":                         "square-terminal",
	"command":                          "reicon-it-terminal-square",
	"loader":                           "loader-circle",
	"code":                             "code-xml",
	"tasks":                            "reicon-ui-tasks",
	"writing":                          "reicon-writing",
	"remix/progress-4-line":            "remix-progress-4",
	"remix/progress-2-line":            "remix-progress-2",
	"remix/progress-6-line":            "remix-progress-6",
	"remix/close-circle-line":          "remix-close-circle-line",
	"remix/checkbox-blank-circle-line": "remix-checkbox-blank-circle-line",
}

// Plugin resolves an icon a plugin names: a bundled icon by its file name,
// alias or "reicon:<group>/<name>", or a brand mark of the AI Logo
// collection as "ailogo:<slug>" with an optional variant suffix
// ("ailogo:claude-color").
func Plugin(name string) (*ui.SVG, error) {
	if brand, found := strings.CutPrefix(name, "ailogo:"); found {
		if mark, _ := pluginLogo(brand); mark != nil {
			return mark, nil
		}
		return nil, fmt.Errorf("plugin icon %q is unavailable", name)
	}
	normalized := strings.TrimSuffix(strings.TrimPrefix(name, "icons/"), ".svg")
	if strings.HasPrefix(normalized, "reicon:") {
		normalized = "reicon-" + strings.ReplaceAll(strings.TrimPrefix(normalized, "reicon:"), "/", "-")
	} else if alias, exists := pluginAliases[normalized]; exists {
		normalized = alias
	}
	if strings.HasPrefix(normalized, "kubai:") {
		return kubaiSVG(normalized)
	}
	if cached, exists := pluginIcons.Load(normalized); exists {
		return cached.(*ui.SVG), nil
	}
	data, err := files.ReadFile("svg/" + normalized + ".svg")
	if err != nil {
		return nil, fmt.Errorf("plugin icon %q is unavailable", name)
	}
	icon, err := ui.ParseSVG(data)
	if err != nil {
		return nil, fmt.Errorf("parse plugin icon %q: %w", name, err)
	}
	cached, _ := pluginIcons.LoadOrStore(normalized, icon)
	return cached.(*ui.SVG), nil
}
