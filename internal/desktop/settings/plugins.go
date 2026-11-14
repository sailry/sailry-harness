package settings

import (
	"slices"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// The Plugins page (settings/plugins*, plugins/*): the inventory's
// controls (check updates, update all, review updates, download,
// install), the installed plugins classified as plugins as resource
// cards, and the dialogs: configuration, install, download from Git,
// the updates review with release notes, and uninstall.

var pluginSettingsMessages = Messages{"plugins_settings_title", "plugins_failed", "plugins_settings_conflict", "plugins_unknown"}

// Update reports whether a package has a newer version.
func Update(p *preview.Package) bool { return p.Latest != "" && p.Latest != p.Version }

// Classified are the installed packages of a kind.
func Classified(ps []preview.Package, kind preview.PackageKind) []int {
	var out []int
	for i := range ps {
		if ps[i].Kind == kind && ps[i].Installed {
			out = append(out, i)
		}
	}
	return out
}

// pluginConfig is the plugin's settings drafted, with the secret action (0
// keep, 1 replace, 2 clear) of each secret.
type pluginConfig struct {
	open       bool
	pending    bool
	generation uint64
	id         string
	// name is the package's, given by the live runtime that opened the
	// dialog: a Desktop package configured while another host is selected
	// is not in that host's list.
	name    string
	fields  []preview.Setting
	secrets map[string]int
	tabs    []viewmodel.SettingsTab
	tab     int
}

// pluginDownload is the Git repository, revision and path, and the download's
// progress while pending.
type pluginDownload struct {
	open                  bool
	repository, rev, path string
	pending               bool
	generation            uint64
	started               time.Time
}

// pluginInstall is a package path and directory.
type pluginInstall struct {
	open           bool
	pending        bool
	generation     uint64
	pkg, directory string
}

// pluginUpdates is the candidates chosen.
type pluginUpdates struct {
	open   bool
	chosen map[string]bool
}

func (m *Model) pkg(id string) *preview.Package {
	for _, list := range [][]preview.Package{m.Packages, m.MarketPlugins} {
		for i := range list {
			if list[i].ID == id {
				return &list[i]
			}
		}
	}
	return nil
}

// compatibility is the Sailry a version needs.
func compatibility(c *ui.Context, p *preview.Package) string {
	if p.Requires == "" {
		return ""
	}
	return i18n.T(c, "plugins_requires_sailry", "version", p.Requires)
}

// tints are the declared tile colors in theme.Tints' order.
var tints = []string{"blue", "violet", "teal", "amber", "gray"}

// ownIcons are the icons declared as SVG markup, parsed once; nil for
// markup in error.
var ownIcons sync.Map

// iconTile is a record's icon on a size DIPs square tile, radius 11 at 36.
// A glyph (a Phosphor or Reicon name, or the record's own SVG) is white,
// 5/9 of the tile, on the gradient of the declared tint, or of the tint the
// name hashes to. A brand mark ("ailogo:<slug>") is drawn on a neutral
// tile, in the brand's colors where the collection has them and in the
// foreground color otherwise. fallback is the glyph of a record declaring
// no icon and of a name nothing draws.
func iconTile(c *ui.Context, name, icon, tint string, fallback *ui.SVG, size float32) {
	tile := ui.Row(c).Size(size, size).Shrink(0).Radius(size * 11 / 36).Center()
	if mark, colored := icons.PluginBrand(icon); mark != nil {
		pal := theme.Of(c)
		tile.Background(pal.Hover).Border(1, pal.Border).TextColor(pal.Foreground).Children(func() {
			if colored {
				ui.Image(c, mark).Size(size*11/18, size*11/18)
			} else {
				ui.Icon(c, mark).FontSize(size * 11 / 18)
			}
		})
		return
	}
	glyph := fallback
	if strings.HasPrefix(icon, "<") {
		parsed, known := ownIcons.Load(icon)
		if !known {
			svg, _ := ui.ParseSVG([]byte(icon))
			parsed, _ = ownIcons.LoadOrStore(icon, svg)
		}
		if svg := parsed.(*ui.SVG); svg != nil {
			glyph = svg
		}
	} else if svg, err := icons.Plugin(icon); err == nil {
		glyph = svg
	}
	accent := kit.Tint(name)
	if i := slices.Index(tints, tint); i >= 0 {
		accent = theme.Tints[i]
	}
	tile.Gradient(accent[0], accent[1], 180).TextColor(theme.OnTint).Children(func() {
		ui.Icon(c, glyph).FontSize(size * 5 / 9)
	})
}

func plugins(c *ui.Context, s *State) {
	m := s.Model
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	shown := Classified(m.Packages, preview.KindPlugin)
	updates, blocked := 0, 0
	for _, i := range shown {
		if p := &m.Packages[i]; Update(p) {
			updates++
			if p.Requires != "" {
				blocked++
			}
		}
	}
	// The header: Check updates, Download and Install; after a check found
	// updates, Update all, and Review updates for the ones a newer Sailry
	// blocks.
	kit.Group(c, "installed", t("plugins_installed"), "", func() {
		kit.SecondaryBusy(c.Key("check"), t("plugins_check_updates"), m.Busy[BusyPluginUpdates]).Disabled(len(shown) == 0 || m.Busy[BusyPluginUpdates]).OnClick(func() {
			if m.PackageActions != nil && m.PackageActions.CheckUpdates != nil {
				m.PackageActions.CheckUpdates(preview.KindPlugin)
				return
			}
			m.checked = true
			switch {
			case updates == 0:
				kit.Toast(c, kit.Done, t("plugins_up_to_date"))
			default:
				kit.Toast(c, kit.Done, t("plugins_updates_found", "count", updates))
			}
		})
		if m.checked && updates > blocked {
			updating := false
			for _, index := range shown {
				updating = updating || m.Busy[BusyPackage+m.Packages[index].ID]
			}
			kit.SecondaryBusy(c.Key("update-all"), t("plugins_update_all"), updating).OnClick(func() {
				if m.PackageActions != nil && m.PackageActions.Update != nil {
					var names []string
					for _, index := range shown {
						if p := m.Packages[index]; Update(&p) && p.Requires == "" {
							names = append(names, p.ID)
						}
					}
					m.PackageActions.Update(names)
					return
				}
				notice(c)
			})
		}
		if m.checked && blocked > 0 {
			kit.Secondary(c.Key("review"), t("plugins_review_updates")).OnClick(func() {
				m.updates = pluginUpdates{open: true, chosen: map[string]bool{}}
			})
		}
		kit.Secondary(c.Key("download"), t("plugins_download")).OnClick(func() {
			m.download = pluginDownload{open: true, generation: m.download.generation + 1}
		})
		kit.Primary(c.Key("install"), t("plugins_install"), false).Children(func() { ui.Text(c, t("plugins_install")).FontSize(14) }).
			OnClick(func() { m.install = pluginInstall{open: true, generation: m.install.generation + 1} })
	}, func() {
		if len(shown) == 0 {
			kit.Result(c, kit.ResultProps{Icon: icons.Puzzle, Title: t("plugins_empty"), Card: true})
			return
		}
		kit.Grid(c, func() {
			for _, i := range shown {
				pluginCard(c, s, &m.Packages[i])
			}
		})
	})
	PackageConfigurationDialog(c, m)
	pluginInstallDialog(c, m)
	pluginDownloadDialog(c, m)
	pluginUpdatesDialog(c, m)
	if p := m.pkg(m.uninstalling); p != nil {
		open := true
		if confirm(c, "uninstall", &open, t("plugins_uninstall"), t("plugins_remove_effect", "name", p.Name), t("plugins_uninstall")) {
			if m.PackageActions != nil && m.PackageActions.Uninstall != nil {
				id := p.ID
				m.PackageActions.Uninstall(id, func(ok bool) {
					if ok && s.Details != nil && s.Details.ID == id {
						s.Details = nil
					}
				})
				m.uninstalling = ""
				return
			}
			p.Installed, p.Enabled = false, false
			if s.Details != nil && s.Details.ID == p.ID {
				s.Details = nil
			}
			kit.Toast(c, kit.Done, t("plugins_uninstalled"))
		}
		if !open {
			m.uninstalling = ""
		}
	}
}

// pluginCard is the plugin's card opening its details, its update, switch and
// menu (information, configure, uninstall).
func pluginCard(c *ui.Context, s *State, p *preview.Package) {
	m := s.Model
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	desc := p.Description
	if msg := compatibility(c, p); msg != "" && Update(p) {
		desc = msg
	}
	busy := m.Busy[BusyPackage+p.ID]
	kit.ResourceCard(c, p.ID, p.Name, desc, func() { iconTile(c, p.Name, p.Icon, p.Tint, icons.Puzzle, kit.TileSize) }, func() { s.ShowDetails(DetailPlugin, p.ID) }, func(bool) {
		if busy {
			// Its update, switch or removal runs: the spinner, and no press.
			kit.BusyAction(c.Key("busy"), p.Name)
		} else if Update(p) && p.Requires == "" {
			kit.Action(c.Key("update"), icons.Download, t("plugins_update_available"), false).OnClick(func() {
				if m.PackageActions != nil && m.PackageActions.Update != nil {
					m.PackageActions.Update([]string{p.ID})
					return
				}
				notice(c)
			})
		}
		if ui.Switch(c.Key("enabled"), &p.Enabled).Label(t("plugins_enabled")+" "+p.Name).Disabled(busy).Changed() && m.PackageActions != nil && m.PackageActions.Toggle != nil {
			m.PackageActions.Toggle(p.ID, p.Enabled)
		}
		kit.MenuButton(c.Key("menu"), "plugin-menu-"+p.ID, func(b ui.Element) {
			kit.IconLook(c, b, icons.Ellipsis, t("more"), "", 28, false)
		}, func(menu *Menu) {
			if menu.Item(t("plugins_information")).Icon(icons.Info).Chosen() {
				s.ShowDetails(DetailPlugin, p.ID)
			}
			if len(p.Settings) > 0 && menu.Item(t("plugins_configure")).Icon(icons.Settings2).Chosen() {
				m.openConfig(p)
			}
			menu.Separator()
			if menu.Item(t("plugins_uninstall")).Icon(icons.Trash).Danger().Disabled(busy).Chosen() {
				m.uninstalling = p.ID
			}
		}, kit.MenuOptions{At: ui.AnchorBottomEnd, Self: ui.AnchorTopEnd})
	})
}

func (m *Model) openConfig(p *preview.Package) {
	if m.PackageActions != nil && m.PackageActions.Configure != nil {
		m.PackageActions.Configure(p.ID)
		return
	}
	m.config = pluginConfig{open: true, generation: m.config.generation + 1, id: p.ID, fields: slices.Clone(p.Settings), secrets: map[string]int{}}
}

// Check validates the configuration draft: the i18n key of the problem, or
// "".
func (e *pluginConfig) Check() string {
	for _, f := range e.fields {
		switch {
		case f.Kind == "secret" && e.secrets[f.Name] == 1 && f.Value == "":
			return "plugins_settings_secret_required"
		case f.Kind == "secret" && f.Required && (e.secrets[f.Name] == 2 || !f.Set && f.Value == ""):
			return "plugins_settings_required"
		case f.Kind == "number" && f.Value != "":
			if _, err := strconv.ParseFloat(f.Value, 64); err != nil {
				return "plugins_settings_number"
			}
		case f.Required && f.Kind != "secret" && strings.TrimSpace(f.Value) == "" && !(f.Kind == "choice" && slices.Contains(f.Options, f.Value)):
			return "plugins_settings_required"
		}
	}
	return ""
}

// PackageConfigurationDialog is one field per setting; a saved secret keeps,
// replaces or clears its value. The Plugins and Connectors pages draw it;
// elsewhere, as a package's own page whose header opens its settings, the
// live runtime's dialog hook does.
func PackageConfigurationDialog(c *ui.Context, m *Model) {
	e := &m.config
	p := m.pkg(e.id)
	if !e.open || p == nil && e.name == "" {
		return
	}
	name := e.name
	if name == "" {
		name = p.Name
	}
	pal := theme.Of(c)
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	title := t("plugins_settings_title")
	kit.Dialog(c.Key("plugin-config"), &e.open, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600)
		ui.Text(c, name+" · "+t("plugins_settings_description")).FontSize(14).TextColor(pal.MutedForeground)
		if len(e.tabs) > 0 {
			titles := make([]string, len(e.tabs))
			for i, tab := range e.tabs {
				titles[i] = tab.Title
			}
			e.tab = min(max(e.tab, 0), len(e.tabs)-1)
			kit.Segmented(c, "tabs", &e.tab, titles, kit.SegmentedProps{Block: true}).Disabled(e.pending)
		}
		ui.Scroll(c.Key("body")).Grow(1).Shrink(1).Gap(16).Children(func() {
			if len(e.fields) == 0 {
				ui.Text(c, t("plugins_settings_none")).FontSize(14).TextColor(pal.MutedForeground)
			}
			// A required setting is checked on Save, as a toast names it,
			// not marked under every field.
			for _, i := range e.visible() {
				f := &e.fields[i]
				if PluginEffortShown(e.fields, *f) {
					continue
				}
				label := f.Title
				kit.Field(c, f.Name, label, "", func() {
					switch f.Kind {
					case "bool":
						on := f.Value == "true"
						if ui.Switch(c.Key("switch"), &on).Label(label).Disabled(e.pending).Changed() {
							f.Value = strconv.FormatBool(on)
						}
					case "choice":
						chosen := slices.Index(f.Options, f.Value)
						if pick(c, "select", label, &chosen, f.Options, e.pending).FillWidth().Changed() {
							f.Value = f.Options[chosen]
						}
					case "model":
						PluginModelField(c, e.fields, f, m.modelOptions(), label, e.pending)
					case "secret":
						action := e.secrets[f.Name]
						if f.Set {
							kit.Segmented(c, "action", &action, []string{t("plugins_settings_keep"), t("plugins_settings_replace"), t("plugins_settings_clear")}, kit.SegmentedProps{Block: true}).Label(label).Disabled(e.pending)
							e.secrets[f.Name] = action
						}
						switch {
						case !f.Set || action == 1:
							ui.TextInput(c.Key("input"), &f.Value).Password().Label(label).Placeholder(t("plugins_settings_input_hint", "field", label)).FillWidth().Disabled(e.pending)
						case action == 0:
							ui.Text(c, t("plugins_settings_configured")).FontSize(14).TextColor(pal.MutedForeground)
						default:
							ui.Text(c, t("plugins_settings_clear_effect")).FontSize(12).TextColor(pal.Warning)
						}
					default:
						ui.TextInput(c.Key("input"), &f.Value).Label(label).Placeholder(t("plugins_settings_input_hint", "field", label)).FillWidth().Disabled(e.pending)
					}
				})
			}
		})
		footer(c, t("settings_save"), func() { e.open = false }, func() {
			if e.pending {
				return
			}
			if problem := e.Check(); problem != "" {
				kit.Toast(c, kit.Failed, t(problem))
				return
			}
			if m.PackageActions != nil && m.PackageActions.SaveSettings != nil {
				e.pending = true
				id, generation := e.id, e.generation
				m.PackageActions.SaveSettings(id, slices.Clone(e.fields), e.secrets, func(ok bool) {
					if e.id == id && e.generation == generation {
						e.pending = false
						if ok {
							e.open = false
							for index := range e.fields {
								if e.fields[index].Kind == "secret" {
									e.fields[index].Value = ""
								}
							}
						}
					}
				})
				return
			}
			if p == nil {
				return
			}
			saved := Stored[[]preview.Setting]{Saved: p.Settings, Draft: e.fields}
			for i := range saved.Draft {
				f := &saved.Draft[i]
				if f.Kind == "secret" {
					f.Set = e.secrets[f.Name] != 2 && (f.Set || f.Value != "")
					f.Value = "" // secrets are never kept
				}
			}
			o := saved.Commit()
			p.Settings = saved.Saved
			report(c, o, Messages{Failed: pluginSettingsMessages.Failed, Conflict: pluginSettingsMessages.Conflict, Uncertain: pluginSettingsMessages.Uncertain})
			e.open = false
		}, e.pending)
	}).Width(min(560, width(c)-48)).Label(title)
}

// pluginInstallDialog is a package path and a working directory; the preview
// installs nothing.
func pluginInstallDialog(c *ui.Context, m *Model) {
	e := &m.install
	if m.PackageActions != nil && m.PackageActions.Install != nil {
		if !e.open {
			return
		}
		title := i18n.T(c, "plugins_install")
		kit.Dialog(c.Key("plugin-install"), &e.open, func() {
			ui.Text(c, title).FontSize(16).FontWeight(600)
			for _, field := range []struct {
				label, hint string
				value       *string
			}{{"plugins_package", "form_package_hint", &e.pkg}, {"plugins_directory", "form_directory_hint", &e.directory}} {
				kit.Field(c, field.label, i18n.T(c, field.label), "", func() {
					input := func() ui.Element {
						return ui.TextInput(c.Key("input"), field.value).Label(i18n.T(c, field.label)).Placeholder(i18n.T(c, field.hint)).Disabled(e.pending)
					}
					if field.value != &e.pkg || m.PackageActions.Browse == nil {
						input().FillWidth()
						return
					}
					// The package is read on this machine before its upload,
					// so its picker browses this machine whichever host is
					// shown.
					ui.Row(c.Key("row")).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
						input().Grow(1).MinWidth(0)
						generation := e.generation
						kit.IconButton(c.Key("browse"), icons.FolderOpen, i18n.T(c, "files_open"), 32, false).Disabled(e.pending).OnClick(func() {
							m.PackageActions.Browse(i18n.T(c, "local_host_name"), strings.TrimSpace(e.directory),
								func() bool { return e.open && !e.pending && e.generation == generation },
								func(path string) { e.pkg = path })
						})
					})
				})
			}
			footer(c, title, func() { e.open = false }, func() {
				if e.pending {
					return
				}
				if strings.TrimSpace(e.pkg) == "" {
					kit.Toast(c, kit.Failed, i18n.T(c, "plugins_package_required"))
					return
				}
				e.pending = true
				generation := e.generation
				m.PackageActions.Install(e.pkg, e.directory, func(ok bool) {
					// The dialog opened since is another install's.
					if e.generation != generation {
						return
					}
					e.pending = false
					if ok {
						e.open = false
					}
				})
			}, e.pending)
		}).Width(min(520, width(c)-48)).Label(title)
		return
	}
	draftDialog(c, "plugin-install", &e.open, "plugins_install", []field{
		{label: "plugins_package", hint: "form_package_hint", value: &e.pkg},
		{label: "plugins_directory", hint: "form_directory_hint", value: &e.directory},
	}, func() string {
		if strings.TrimSpace(e.pkg) == "" {
			return "plugins_package_required"
		}
		notice(c)
		return ""
	})
}

// pluginDownloadDialog is a Git repository, a revision and a path, then
// Install, Installing with its progress, or Retry.
func pluginDownloadDialog(c *ui.Context, m *Model) {
	e := &m.download
	if !e.open {
		return
	}
	pal := theme.Of(c)
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	title := t("plugins_download")
	kit.Dialog(c.Key("plugin-download"), &e.open, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600)
		for _, f := range []struct {
			label, hint string
			value       *string
		}{{"skills_repository", "skills_repository_hint", &e.repository}, {"skills_revision", "skills_revision_hint", &e.rev}, {"plugins_path", "plugins_path_hint", &e.path}} {
			kit.Field(c, f.label, t(f.label), "", func() {
				ui.TextInput(c.Key("input"), f.value).Label(t(f.label)).Placeholder(t(f.hint)).FillWidth().Disabled(e.pending)
			})
		}
		if e.pending {
			if m.PackageActions != nil {
				ui.Column(c.Key("progress")).Gap(6).Children(func() {
					ui.Progress(c, -1).FillWidth().Height(4)
					ui.Text(c, t("plugins_preparing_package")).FontSize(12).TextColor(pal.MutedForeground)
				})
			} else {
				// The preview's download: progress over two seconds, then the
				// notice that nothing was fetched.
				frac := float64(c.Now().Sub(e.started)) / float64(2*time.Second)
				if frac >= 1 {
					e.pending, e.open = false, false
					notice(c)
				} else {
					c.After(time.Second / kit.FPS)
					ui.Column(c.Key("progress")).Gap(6).Children(func() {
						ui.Progress(c, frac).FillWidth().Height(4)
						ui.Text(c, t("plugins_preparing_package")).FontSize(12).TextColor(pal.MutedForeground)
					})
				}
			}
		}
		label := t("plugins_install")
		if e.pending {
			label = t("plugins_installing")
		}
		ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Children(func() {
			kit.Secondary(c.Key("cancel"), t("settings_cancel")).OnClick(func() {
				if m.PackageActions != nil && m.PackageActions.CancelDownload != nil {
					m.PackageActions.CancelDownload()
				}
				e.open, e.pending = false, false
			})
			kit.Primary(c.Key("ok"), label, e.pending).Children(func() { ui.Text(c, label).FontSize(14) }).OnClick(func() {
				if !strings.HasPrefix(e.repository, "https://") {
					kit.Toast(c, kit.Failed, t("skills_invalid"))
					return
				}
				e.pending, e.started = true, c.Now()
				if m.PackageActions != nil && m.PackageActions.Download != nil {
					generation := e.generation
					m.PackageActions.Download(e.repository, e.rev, e.path, func(ok bool) {
						if e.generation != generation {
							return
						}
						e.pending = false
						if ok {
							e.open = false
						}
					})
				}
			})
		})
	}).Width(min(520, width(c)-48)).Label(title)
}

// pluginUpdatesDialog is each candidate with its version, what it needs and
// its release notes, chosen by a check; Update (n) and Later.
func pluginUpdatesDialog(c *ui.Context, m *Model) {
	e := &m.updates
	if !e.open {
		return
	}
	pal := theme.Of(c)
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	title := t("plugins_updates_title")
	kit.Dialog(c.Key("plugin-updates"), &e.open, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600)
		ui.Scroll(c.Key("body")).Grow(1).Shrink(1).Gap(12).Children(func() {
			for i := range m.Packages {
				p := &m.Packages[i]
				if !p.Installed || !Update(p) {
					continue
				}
				ui.Column(c.Key(p.ID)).Gap(6).Padding(12).Border(1, pal.Border).Radius(pal.RadiusLg).Children(func() {
					ui.Row(c).Gap(12).Children(func() {
						on := e.chosen[p.ID]
						if ui.Checkbox(c.Key("check"), &on, "").Label(p.Name).Disabled(p.Requires != "").Changed() {
							e.chosen[p.ID] = on
						}
						iconTile(c, p.Name, p.Icon, p.Tint, icons.Puzzle, 28)
						ui.Column(c).Grow(1).Gap(2).Children(func() {
							ui.Text(c, p.Name).FontSize(14).FontWeight(500)
							ui.Text(c, t("plugins_update_version", "version", p.Latest)).FontSize(12).TextColor(pal.MutedForeground)
						})
						if p.Requires != "" {
							kit.Secondary(c.Key("sailry"), t("plugins_update_sailry")).Height(28).OnClick(func() {
								if m.PackageActions != nil && m.PackageActions.About != nil {
									m.PackageActions.About()
									return
								}
								notice(c)
							})
						}
					})
					if msg := compatibility(c, p); msg != "" {
						ui.Text(c, msg).FontSize(12).TextColor(pal.Warning)
					}
					releaseNotes(c, p.Notes)
					addedPermissions(c, p)
				})
			}
		})
		n := 0
		for _, on := range e.chosen {
			if on {
				n++
			}
		}
		ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Children(func() {
			kit.Secondary(c.Key("later"), t("plugins_updates_later")).OnClick(func() { e.open = false })
			label := t("plugins_updates_selected", "count", n)
			kit.Primary(c.Key("ok"), label, n == 0).Children(func() { ui.Text(c, label).FontSize(14) }).OnClick(func() {
				e.open = false
				if m.PackageActions != nil && m.PackageActions.Update != nil {
					var names []string
					for name, chosen := range e.chosen {
						if chosen {
							names = append(names, name)
						}
					}
					m.PackageActions.Update(names)
					return
				}
				notice(c)
			})
		})
	}).Width(min(560, width(c)-48)).Label(title)
}

// addedPermissions names what an update asks that the installed version
// does not have.
func addedPermissions(c *ui.Context, p *preview.Package) {
	if len(p.AddedPermissions) == 0 {
		return
	}
	pal := theme.Of(c)
	ui.Row(c.Key("added-permissions")).Gap(8).Padding(8, 12).AlignItems(ui.Start).Children(func() {
		ui.Icon(c, icons.Shield).FontSize(16).Shrink(0).TextColor(pal.Warning)
		ui.Text(c, i18n.T(c, "market_details_added_permissions")+": "+strings.Join(labels(c, p.AddedPermissions), ", ")).FontSize(14).Grow(1).Shrink(1).TextColor(pal.Warning)
	})
}

// releaseNotes is the notes' lines as plain text, list markers dropped.
func releaseNotes(c *ui.Context, notes string) {
	if strings.TrimSpace(notes) == "" {
		return
	}
	ui.Column(c.Key("notes")).Gap(8).Padding(8, 12).Children(func() {
		for i, line := range strings.Split(notes, "\n") {
			line = strings.TrimPrefix(strings.TrimSpace(line), "- ")
			if line != "" {
				ui.Text(c.Key(i), line).FontSize(14).Selectable()
			}
		}
	})
}

// visible are the indexes of the fields the chosen tab shows, as the
// reference's configuration tabs: its fields in its order, only its Enabled
// field while that is off, then the fields no tab holds. Without tabs, all.
func (e *pluginConfig) visible() []int {
	index := func(name string) int {
		return slices.IndexFunc(e.fields, func(f preview.Setting) bool { return f.Name == name })
	}
	if len(e.tabs) == 0 {
		out := make([]int, len(e.fields))
		for i := range out {
			out[i] = i
		}
		return out
	}
	tab := e.tabs[min(max(e.tab, 0), len(e.tabs)-1)]
	on := tab.Enabled == "" || index(tab.Enabled) >= 0 && e.fields[index(tab.Enabled)].Value == "true"
	var out []int
	for _, name := range tab.Fields {
		if i := index(name); i >= 0 && (on || name == tab.Enabled) {
			out = append(out, i)
		}
	}
	for i, f := range e.fields {
		if !slices.ContainsFunc(e.tabs, func(tab viewmodel.SettingsTab) bool { return slices.Contains(tab.Fields, f.Name) }) {
			out = append(out, i)
		}
	}
	return out
}

// PluginEffortShown reports whether f is an effort a model field of fields
// chooses in its own picker: the form draws no field of its own for it.
func PluginEffortShown(fields []preview.Setting, f preview.Setting) bool {
	return f.Kind == "effort" && slices.ContainsFunc(fields, func(model preview.Setting) bool {
		return model.Kind == "model" && model.Name == f.Model
	})
}

// PluginModelField is a package's model setting: kit.ModelSelect of the
// models it offers, by their names. Where an effort setting follows the
// field, the picker offers each model's efforts after Follow model, the
// effort "", and choosing writes both, so the form has no effort row.
// PluginEffortShown tells the forms which fields it draws this way.
func PluginModelField(c *ui.Context, fields []preview.Setting, f *preview.Setting, known []kit.ModelOption, label string, disabled bool) {
	models := kit.ModelsOffered(known, f.Options)
	var effort *preview.Setting
	for i := range fields {
		if fields[i].Kind == "effort" && fields[i].Model == f.Name {
			effort = &fields[i]
		}
	}
	props := kit.ModelSelectProps{Label: label, Placeholder: i18n.T(c, "composer_select_model"), Models: models,
		Model: f.Value, Disabled: disabled, OnChange: func(id, _ string) { f.Value = id }}
	if effort != nil {
		// A level the chosen model does not take, as the schemas' "none",
		// follows the model: resolveModel would reject it.
		if !slices.Contains(effort.Efforts[f.Value], effort.Value) {
			effort.Value = ""
		}
		for i := range models {
			models[i].Efforts = effort.Efforts[models[i].ID]
		}
		props.Effort, props.AutoEffort = effort.Value, i18n.T(c, "plugins_settings_follow_model")
		props.OnChange = func(id, level string) { f.Value, effort.Value = id, level }
	}
	kit.ModelSelect(c.Key("select"), props).FillWidth()
}
