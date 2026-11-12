package settings

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// PythonPreparation is a package whose Python dependencies are being
// prepared: Stage is creating, installing or validating.
type PythonPreparation struct{ Package, Stage string }

// runtimePage is the bundled Python and its package index, a custom URL when
// chosen.
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
					if m.PackageActions != nil && m.PackageActions.SavePython != nil {
						m.PackageActions.SavePython()
					} else {
						py.Commit()
					}
				}
			})
			if py.Draft.Index == len(options)-1 {
				title := i18n.T(c, "runtime_python_custom_url")
				kit.WideRow(c, "custom", title, "", func() {
					if ui.TextInput(c.Key("input"), &py.Draft.Custom).Label(title).Placeholder("https://").FillWidth().Disabled(py.Pending).Submitted() {
						if m.PackageActions != nil && m.PackageActions.SavePython != nil {
							m.PackageActions.SavePython()
						} else {
							py.Commit()
						}
					}
				})
			}
			if len(m.PythonPreparing) > 0 {
				p := theme.Of(c)
				kit.Row(c, "preparing", i18n.T(c, "runtime_python_preparing"), "", func() {
					ui.Column(c.Key("packages")).Gap(4).AlignItems(ui.End).Children(func() {
						for _, preparation := range m.PythonPreparing {
							ui.Row(c.Key(preparation.Package)).Gap(8).MinWidth(0).Children(func() {
								kit.Busy(c)
								ui.Text(c, preparation.Package).FontSize(14).SingleLine().TextColor(p.MutedForeground)
								ui.Text(c, i18n.T(c, "python_"+preparation.Stage)).FontSize(14).SingleLine().TextColor(p.MutedForeground)
							})
						}
					})
				})
			}
		})
	})
}
