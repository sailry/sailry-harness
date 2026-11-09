package settings

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/kit"
)

// runtimePage is runtime/view.rs: the bundled Python and its package
// index, a custom URL when chosen.
func runtimePage(c *ui.Context, m *Model) {
	py := &m.Python
	kit.Group(c, "python", i18n.T(c, "runtime_python"), "", nil, func() {
		kit.Card(c, func() {
			kit.Row(c, "version", i18n.T(c, "runtime_python_version"), "", func() {
				ui.Text(c, m.PythonVersion).FontSize(14)
				kit.Badge(c, i18n.T(c, "runtime_python_bundled"), kit.Neutral)
			})
			options := append([]string(nil), m.Indexes...)
			options[len(options)-1] = i18n.T(c, "runtime_python_custom")
			title := i18n.T(c, "runtime_python_index")
			kit.WideRow(c, "index", title, "", func() {
				if pick(c, "select", title, &py.Draft.Index, options, py.Pending).Fill().Changed() && py.Draft.Index != len(options)-1 {
					py.Commit()
				}
			})
			if py.Draft.Index == len(options)-1 {
				title := i18n.T(c, "runtime_python_custom_url")
				kit.WideRow(c, "custom", title, "", func() {
					if ui.TextInput(c.Key("input"), &py.Draft.Custom).Label(title).Placeholder("https://").FillWidth().Submitted() {
						py.Commit()
					}
				})
			}
		})
	})
}
