package live

import (
	"errors"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) readInstalledThemes() {
	generation := r.packages.themeGeneration.Add(1)
	r.async(func() {
		installed, err := r.packages.profile.Installed(r.ctx)
		r.enqueue(func(c *ui.Context) {
			if generation != r.packages.themeGeneration.Load() {
				return
			}
			if err != nil {
				r.packageReport(c, err)
				return
			}
			r.packages.themes = map[string]theme.Snapshot{}
			var packages []*theme.Package
			for _, snapshot := range installed {
				r.packages.themes[snapshot.Package.ID] = snapshot
				packages = append(packages, snapshot.Package)
			}
			theme.SetInstalled(packages)
			r.projectMarketThemes()
		})
	})
}

func (r *Runtime) readThemeCatalog(force bool) {
	root := r.localRuntime()
	state := root.packages
	if state.themeBusy["catalog"] {
		return
	}
	state.themeBusy["catalog"] = true
	state.themeRead = true
	if root.App.Settings.MarketKind == 2 {
		root.App.Settings.MarketLoad.Loading, root.App.Settings.MarketLoad.Loaded = true, false
	}
	r.async(func() {
		listing, err := state.market.List(r.ctx, force)
		r.enqueue(func(c *ui.Context) {
			delete(state.themeBusy, "catalog")
			if root.App.Settings.MarketKind == 2 {
				root.App.Settings.MarketLoad.Loading, root.App.Settings.MarketLoad.Loaded = false, true
			}
			if err != nil {
				kit.Toast(c, kit.Failed, i18n.T(c, "market_themes_failed"))
				return
			}
			state.themeCatalog = listing
			r.projectMarketThemes()
			if listing.Unavailable {
				kit.Toast(c, kit.Failed, i18n.T(c, "market_themes_failed"))
			} else if force {
				key := "appearance_up_to_date"
				for _, entry := range listing.Themes {
					if installed := theme.Find(entry.ID); installed.ID == entry.ID && entry.Newer(installed) {
						key = "appearance_updates_available"
						break
					}
				}
				kit.Toast(c, kit.Done, i18n.T(c, key))
			}
		})
	})
}

func (r *Runtime) projectMarketThemes() {
	root := r.localRuntime()
	if root.packages == nil {
		return
	}
	listing := root.packages.themeCatalog
	rows := make([]viewmodel.MarketTheme, 0, len(listing.Themes))
	for _, entry := range listing.Themes {
		pkg := theme.Find(entry.ID)
		installed := pkg.ID == entry.ID
		row := viewmodel.MarketTheme{ID: entry.ID, Name: entry.Name, Description: stringValue(entry.Description), Installed: installed, Builtin: installed, Current: root.App.Theme == entry.ID}
		if description := entry.DescriptionLocales[i18n.Match(r.packageLocale())]; description != "" {
			row.Description = description
		}
		for _, mode := range entry.Modes {
			row.Dark = row.Dark || mode == "dark"
			row.Light = row.Light || mode == "light"
		}
		if installed {
			row.Update = entry.Newer(pkg)
		}
		if _, exists := root.packages.themes[entry.ID]; exists {
			row.Builtin = false
		}
		rows = append(rows, row)
	}
	for _, pkg := range theme.Packages {
		if pkg == theme.Builtin {
			continue
		}
		present := false
		for _, row := range rows {
			present = present || row.ID == pkg.ID
		}
		if !present {
			rows = append(rows, viewmodel.MarketTheme{ID: pkg.ID, Name: pkg.Name, Description: pkg.Description, Dark: pkg.Dark != nil, Light: pkg.Light != nil, Installed: true, Current: root.App.Theme == pkg.ID})
		}
	}
	root.App.Settings.Themes = rows
}

func (r *Runtime) installMarketTheme(id string) {
	root := r.localRuntime()
	state := root.packages
	if state.themeBusy[id] {
		return
	}
	selection := state.themeCatalog.Selection(id)
	pkg := theme.Find(id)
	if pkg.ID == id && (selection == nil || !selection.Entry.Newer(pkg)) {
		root.App.Theme = id
		return
	}
	if selection == nil {
		root.enqueue(func(c *ui.Context) { root.packageReport(c, errors.New("the immutable theme selection is unavailable")) })
		return
	}
	state.themeBusy[id] = true
	var expected *string
	if installed, exists := state.themes[id]; exists {
		value := installed.Digest
		expected = &value
	}
	root.async(func() {
		snapshot, err := state.market.Download(root.ctx, *selection)
		if err == nil {
			err = state.profile.Install(root.ctx, snapshot, expected)
		}
		root.enqueue(func(c *ui.Context) {
			delete(state.themeBusy, id)
			root.themeReport(c, err, "appearance_import_done", "appearance_save_failed", "appearance_save_uncertain")
		})
	})
}

func (r *Runtime) importTheme() {
	if r.attachments.native.PickFiles == nil {
		return
	}
	r.attachments.native.PickFiles(func(paths []string) {
		if len(paths) == 0 {
			return
		}
		selected := paths[0]
		r.async(func() {
			snapshot, err := theme.ReadSnapshot(r.ctx, selected)
			r.enqueue(func(c *ui.Context) {
				if err != nil {
					kit.Toast(c, kit.Failed, i18n.T(c, "appearance_import_invalid"))
					return
				}
				preview := &settings.ThemeImport{Package: snapshot.Package, Open: true}
				var expected *string
				if installed, exists := r.packages.themes[snapshot.Package.ID]; exists {
					value := installed.Digest
					expected = &value
				}
				preview.Install = func(done func(bool)) {
					r.async(func() {
						err := r.packages.profile.Install(r.ctx, snapshot, expected)
						r.enqueue(func(c *ui.Context) {
							r.themeReport(c, err, "appearance_import_done", "appearance_save_failed", "appearance_save_uncertain")
							done(err == nil)
						})
					})
				}
				r.App.Settings.AppearanceActions.Preview = preview
			})
		})
	})
}

func (r *Runtime) exportTheme(pkg *theme.Package) {
	snapshot, exists := r.packages.themes[pkg.ID]
	if !exists || r.attachments.native.SaveFile == nil {
		return
	}
	r.attachments.native.SaveFile(pkg.ID+".zip", func(path string) {
		if path == "" {
			return
		}
		r.async(func() {
			destination, err := prepareDownloadDestination(path)
			if err == nil {
				defer destination.close()
				err = snapshot.WriteZIP(destination.file)
				if err == nil {
					err = destination.publish(r.ctx)
				}
			}
			r.enqueue(func(c *ui.Context) {
				r.themeReport(c, err, "appearance_export_done", "appearance_export_failed", "directory_outcome_unknown")
			})
		})
	})
}

func (r *Runtime) uninstallTheme(pkg *theme.Package, done func(bool)) {
	snapshot, exists := r.packages.themes[pkg.ID]
	if !exists {
		done(false)
		return
	}
	r.async(func() {
		err := r.packages.profile.Remove(r.ctx, pkg.ID, snapshot.Digest)
		r.enqueue(func(c *ui.Context) {
			if err == nil {
				if r.App.Theme == pkg.ID {
					r.App.Theme = ""
				}
			}
			r.themeReport(c, err, "appearance_removed", "appearance_remove_failed", "workspace_action_unknown")
			done(err == nil)
		})
	})
}

func (r *Runtime) themeReport(c *ui.Context, err error, succeeded, failed, unknown string) {
	if err == nil {
		kit.Toast(c, kit.Done, i18n.T(c, succeeded))
		return
	}
	if errors.Is(err, theme.ErrConflict) {
		err = fileActionFault(protocol.ErrorCodeRevisionConflict, "the installed theme changed")
	} else if errors.Is(err, theme.ErrPublication) {
		err = fileActionFault(protocol.ErrorCodeOutcomeUnknown, "theme publication could not be confirmed")
	}
	r.reportKeys(c, err, failed, failed, unknown)
}
