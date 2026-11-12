package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) packageDialogs(c *ui.Context) {
	r.skillSelectionDialog(c)
}

func (r *Runtime) skillSelectionDialog(c *ui.Context) {
	actions := r.App.Settings.PackageActions
	if actions == nil || actions.Selection == nil || !actions.Selection.Open {
		return
	}
	selection := actions.Selection
	title := i18n.T(c, "skills_install")
	width, height := c.Size()
	kit.Dialog(c.Key("skill-selection"), &selection.Open, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600)
		ui.Scroll(c.Key("candidates")).MaxHeight(min(400, height*0.55)).Gap(8).Children(func() {
			for index := range selection.Candidates {
				candidate := &selection.Candidates[index]
				ui.Row(c.Key(index)).Padding(8).Gap(8).Children(func() {
					ui.Checkbox(c.Key("selected"), &candidate.Selected, candidate.Name).Disabled(selection.Pending)
					ui.Text(c, candidate.Description).FontSize(14).TextColor(theme.Of(c).MutedForeground).Grow(1).Shrink(1)
				})
			}
		})
		ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Children(func() {
			kit.Secondary(c.Key("cancel"), i18n.T(c, "settings_cancel")).Disabled(selection.Pending).OnClick(func() { selection.Open = false })
			var indices []int
			for index, candidate := range selection.Candidates {
				if candidate.Selected {
					indices = append(indices, index)
				}
			}
			kit.PrimaryBusy(c.Key("install"), i18n.T(c, "plugins_install"), len(indices) == 0, selection.Pending).OnClick(func() {
				selection.Pending = true
				selection.Install(indices, func(ok bool) {
					selection.Pending = false
					if ok {
						selection.Open = false
					}
				})
			})
		})
	}).Width(min(560, width-48)).Label(title)
	if !selection.Open && !selection.Pending && selection.Cancel != nil {
		selection.Cancel()
		selection.Cancel = nil
	}
}

func (r *Runtime) pluginReferences() []composer.Reference {
	if r.packages == nil {
		r.packages = newPackageState()
	}
	var rows []composer.Reference
	for _, summary := range r.snapshot().Plugins {
		if !summary.Enabled || summary.Compatibility.Kind != "compatible" {
			continue
		}
		if info, exists := r.packages.infos[summary.Name]; exists {
			if info.Extension == nil || info.Extension.Scope == protocol.PluginScopeDesktop {
				continue
			}
			view := packageView(info, r.packageLocale())
			rows = append(rows, composer.Reference{ID: summary.Name, Kind: composer.Plugin, Label: view.Name, Description: view.Description, Group: "reference_group_plugins", Icon: icons.Puzzle})
		} else if !r.packages.reads[summary.Name] && r.packages.failures[summary.Name] != packageVersion(summary) {
			r.readPackage(summary.Name, packageVersion(summary), nil)
		}
	}
	return rows
}
