package settings

import (
	"fmt"
	"maps"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The Teams page: the child concurrency, the Teams and Roles tabs with New,
// the Default team and the saved teams, the roles; the team editor (name,
// description, lead model and effort, lead prompt, members) and the role
// editor (presets, name, identifier, description, model and effort, read-
// only, instructions).

var concurrencyMessages = Messages{"", "teams_settings_failed", "teams_settings_conflict", "teams_settings_unknown"}

// rolePresets are the role editor's presets: label, description and prompt
// keys.
var rolePresets = [][3]string{
	{"role_preset_review", "role_review_description", "role_prompt_review"},
	{"role_preset_implement", "role_implement_description", "role_prompt_implement"},
	{"role_preset_test", "role_test_description", "role_prompt_test"},
	{"role_preset_docs", "role_docs_description", "role_prompt_docs"},
}

// teamEditor edits a team: the Default team (default), a new one
// (team.ID == "") or a saved one.
type teamEditor struct {
	open            bool
	def             bool
	team            preview.Team
	pending, locked bool
	request         *bool
}

// roleEditor edits a role, a new one when role.ID == "".
type roleEditor struct {
	open            bool
	role            preview.Role
	pending, locked bool
	request         *bool
}

// modelChoices are the channels' models as "channel/model" with their
// labels, after none (the "" choice labeled follow).
func (m *Model) modelChoices(c *ui.Context, follow string) (ids, names []string) {
	ids, names = []string{""}, []string{i18n.T(c, follow)}
	for _, ch := range m.Channels {
		if !ch.Enabled {
			continue
		}
		for _, x := range ch.Models {
			ids = append(ids, ch.ID+"/"+x.ID)
			names = append(names, ch.Name+" · "+x.Name)
		}
	}
	return ids, names
}

// modelOptions are the enabled channels' models as kit.ModelSelect offers
// them, named "channel/model" as modelChoices names them, each under its
// channel with the channel's mark.
func (m *Model) modelOptions() []kit.ModelOption {
	var out []kit.ModelOption
	for i := range m.Channels {
		ch := &m.Channels[i]
		if !ch.Enabled {
			continue
		}
		for _, x := range ch.Models {
			out = append(out, kit.ModelOption{ID: ch.ID + "/" + x.ID, Name: x.Name, Provider: ch.Name, Mark: channelMark(ch),
				Efforts: x.Efforts, DefaultEffort: x.DefaultEffort})
		}
	}
	return out
}

// modelFeatures are the features a model declares.
func (m *Model) modelFeatures(id string) []preview.ModelFeature {
	ch, model, _ := strings.Cut(id, "/")
	if x := m.channel(ch); x != nil {
		for _, y := range x.Models {
			if y.ID == model {
				return y.Features
			}
		}
	}
	return nil
}

// modelSummary is the model's label, or follow.
func (m *Model) modelSummary(c *ui.Context, id, follow string) string {
	ids, names := m.modelChoices(c, follow)
	if i := slices.Index(ids, id); i >= 0 {
		return names[i]
	}
	return id
}

func roleCount(c *ui.Context, n int) string {
	if n == 1 {
		return i18n.T(c, "team_role_one")
	}
	return i18n.T(c, "team_role_count", "count", n)
}

func (m *Model) role(id string) *preview.Role {
	for i := range m.Roles {
		if m.Roles[i].ID == id {
			return &m.Roles[i]
		}
	}
	return nil
}

func teams(c *ui.Context, m *Model) {
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	ui.Column(c.Key("teams")).Gap(16).Children(func() {
		kit.Card(c, func() {
			kit.WideRow(c, "concurrency", t("teams_concurrency"), t("teams_concurrency_description"), func() {
				shown := min(16, max(1, m.Concurrency.Draft))
				if ui.StepSlider(c.Key("slider"), &shown, 1, 16, 1).Label(t("teams_concurrency")).Grow(1).Disabled(m.Concurrency.Pending || m.Live && m.TeamActions == nil).Changed() {
					m.Concurrency.Draft = shown
					if m.Live && m.TeamActions != nil {
						m.TeamActions.SaveConcurrency()
					} else {
						report(c, m.Concurrency.Commit(), concurrencyMessages)
					}
				}
				if m.Live && m.Concurrency.Pending {
					kit.Busy(c) // the limit is being saved
				}
				ui.Textf(c, "%.0f", m.Concurrency.Draft).FontSize(14).Width(24).TextAlign(ui.End)
			})
		})
		ui.Row(c.Key("bar")).Gap(12).Children(func() {
			kit.Segmented(c, "tabs", &m.TeamTab, []string{t("settings_teams"), t("teams_roles")}, kit.SegmentedProps{}).Label(t("settings_teams"))
			ui.Spacer(c)
			label := t("team_new")
			if m.TeamTab == 1 {
				label = t("role_new")
			}
			iconButton(c, "new", icons.Plus, label, true, false).OnClick(func() {
				if m.TeamTab == 0 {
					m.teamEdit = teamEditor{open: true}
				} else {
					m.roleEdit = roleEditor{open: true}
				}
			})
		})
		if m.TeamTab == 0 {
			teamRows(c, m)
		} else {
			roleRows(c, m)
		}
	})
	teamDialog(c, m)
	roleDialog(c, m)
	if tm := m.team(m.teamRemoving); tm != nil {
		open := true
		if confirm(c, "delete-team", &open, t("team_delete"), t("team_delete_prompt", "name", tm.Name), t("settings_delete")) {
			if m.Live && m.TeamActions != nil {
				// The editor it was deleted from closes; one opened since for
				// another team keeps its draft.
				id := tm.ID
				m.TeamActions.RemoveTeam(*tm, func(outcome kit.Outcome) {
					if outcome != kit.Done {
						return
					}
					if m.teamEdit.team.ID == id {
						m.teamEdit.open = false
					}
					if m.teamRemoving == id {
						m.teamRemoving = ""
					}
				})
			} else {
				m.Teams = slices.DeleteFunc(m.Teams, func(x preview.Team) bool { return x.ID == tm.ID })
				m.teamEdit.open = false
				kit.Toast(c, kit.Done, t("team_deleted"))
			}
		}
		if !open {
			m.teamRemoving = ""
		}
	}
	if r := m.role(m.roleRemoving); r != nil {
		open := true
		if confirm(c, "delete-role", &open, t("settings_delete"), t("teams_role_delete_prompt", "name", r.Name), t("settings_delete")) {
			if m.Live && m.TeamActions != nil {
				id := r.ID
				m.TeamActions.RemoveRole(*r, func(outcome kit.Outcome) {
					if outcome == kit.Done && m.roleRemoving == id {
						m.roleRemoving = ""
					}
				})
			} else {
				m.Roles = slices.DeleteFunc(m.Roles, func(x preview.Role) bool { return x.ID == r.ID })
				for i := range m.Teams {
					m.Teams[i].Members = slices.DeleteFunc(m.Teams[i].Members, func(id string) bool { return id == r.ID })
				}
				kit.Toast(c, kit.Done, t("teams_role_deleted"))
			}
		}
		if !open {
			m.roleRemoving = ""
		}
	}
}

func (m *Model) team(id string) *preview.Team {
	for i := range m.Teams {
		if id != "" && m.Teams[i].ID == id {
			return &m.Teams[i]
		}
	}
	return nil
}

// teamRows is the Default team, then the saved teams, each with its members'
// marks; clicking opens the editor.
func teamRows(c *ui.Context, m *Model) {
	p := theme.Of(c)
	row := func(key, name, desc, meta string, builtin bool, members []string, open func()) {
		kit.Bleed(c, "row:"+key, func() {
			b := ui.ButtonBase(c.Key(key)).Label(name).Padding(12, 16).Margin(0, -16).Gap(12).Justify(ui.Start).Cursor(ui.CursorPointer).OnClick(open)
			if b.Hovered() {
				b.Background(p.Hover)
			}
			b.Children(func() {
				faces := make([]kit.TeamMember, 0, len(members))
				for _, id := range members {
					if r := m.role(id); r != nil {
						faces = append(faces, kit.TeamMember{ID: r.ID, Avatar: r.Avatar})
					}
				}
				kit.MediaRow(c, name, desc, func() { kit.TeamAvatar(c, key, faces, kit.TileSize) }, func() {
					if builtin {
						kit.Tag(c, i18n.T(c, "team_builtin"))
					}
				})
				ui.Row(c).Shrink(0).Gap(4).Children(func() {
					for i, f := range faces[:min(5, len(faces))] {
						kit.RoleAvatar(c.Key(i), f.ID, f.Avatar, 20)
					}
				})
				ui.Text(c, meta).FontSize(14).Shrink(0).TextColor(p.MutedForeground)
			})

		})
	}
	kit.Card(c, func() {
		all := make([]string, len(m.Roles))
		for i, r := range m.Roles {
			all[i] = r.ID
		}
		row("default", i18n.T(c, "team_default"), i18n.T(c, "team_default_description"), roleCount(c, len(all)), true, all, func() {
			m.teamEdit = teamEditor{open: true, def: true, team: preview.Team{Name: i18n.T(c, "team_default"), Members: all}}
		})
		for i := range m.Teams {
			tm := &m.Teams[i]
			meta := roleCount(c, len(tm.Members)) + " · " + m.modelSummary(c, tm.Lead, "team_follow_conversation")
			row(tm.ID, tm.Name, tm.Description, meta, false, tm.Members, func() {
				x := *tm
				x.Members = slices.Clone(tm.Members)
				m.teamEdit = teamEditor{open: true, team: x}
			})
		}
	})
}

// roleRows is each role with its model, and its edit and delete actions.
func roleRows(c *ui.Context, m *Model) {
	if len(m.Roles) == 0 {
		kit.Result(c, kit.ResultProps{Icon: icons.Bot, Title: i18n.T(c, "teams_roles_empty"), Card: true})
		return
	}
	kit.Card(c, func() {
		for i := range m.Roles {
			r := &m.Roles[i]
			desc := m.modelSummary(c, r.Model, "role_inherit")
			if r.ReadOnly {
				desc += " · " + i18n.T(c, "teams_read_only")
			}
			kit.Entry(c, r.ID, func() {
				kit.MediaRow(c, r.Name, desc, func() { kit.RoleAvatar(c, r.ID, r.Avatar, kit.TileSize) }, nil)
			}, nil, func() {
				kit.Action(c.Key("edit"), icons.Pencil, i18n.T(c, "settings_edit"), false).OnClick(func() { m.roleEdit = roleEditor{open: true, role: *r} })
				kit.Action(c.Key("delete"), icons.Trash, i18n.T(c, "settings_delete"), false).OnClick(func() { m.roleRemoving = r.ID })
			})
		}
	})
}

// modelField is the model select of "" (labeled follow) and the channels'
// models, its efforts after Auto beside them in its popover, with one
// select per feature the model declares after it. Another model starts
// from Auto. A feature follows (Auto, features nil), is off (absent from
// the map) or takes one of its options.
func modelField(c *ui.Context, m *Model, key, label, follow string, model, effort *string, features *map[string]string) {
	kit.Field(c, key, label, "", func() {
		ui.Row(c).Gap(8).Wrap().Children(func() {
			kit.ModelSelect(c.Key("model"), kit.ModelSelectProps{Label: label, None: i18n.T(c, follow), Models: m.modelOptions(),
				Model: *model, Effort: *effort, AutoEffort: i18n.T(c, "role_effort_inherit"),
				OnChange: func(id, level string) {
					if id != *model {
						level, *features = "", nil
					}
					*model, *effort = id, level
				}}).Grow(1)
			for _, f := range m.modelFeatures(*model) {
				shown := []string{i18n.T(c, "role_effort_inherit"), i18n.T(c, "composer_feature_off")}
				at := 0
				if *features != nil {
					at = 1
				}
				for i, o := range f.Options {
					name := o.Name
					if f.ID == "speed" && o.ID == "fast" {
						name = i18n.T(c, "composer_feature_fast")
					}
					shown = append(shown, name)
					if *features != nil && (*features)[f.ID] == o.ID {
						at = i + 2
					}
				}
				if pick(c, "feature-"+f.ID, f.Name, &at, shown, false).Width(140).Changed() {
					switch at {
					case 0:
						*features = nil
					default:
						set := maps.Clone(*features)
						if set == nil {
							set = map[string]string{}
						}
						delete(set, f.ID)
						if at > 1 {
							set[f.ID] = f.Options[at-2].ID
						}
						*features = set
					}
				}
			}
		})
	})
}

// teamDialog is teams/editor: name, description, lead model, lead prompt
// and the members; the Default team only shows its members. Its footer
// duplicates, deletes, cancels and saves.
func teamDialog(c *ui.Context, m *Model) {
	e := &m.teamEdit
	if !e.open {
		return
	}
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	title := t("team_edit")
	if e.team.ID == "" && !e.def {
		title = t("team_new")
	}
	kit.Dialog(c.Key("team-editor"), &e.open, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600)
		ui.Scroll(c.Key("body")).Grow(1).Shrink(1).Gap(16).Disabled(e.pending || e.locked).Children(func() {
			if e.def {
				ui.Text(c, t("team_default_description")).FontSize(14).TextColor(theme.Of(c).MutedForeground)
			} else {
				kit.Field(c, "name", t("settings_name"), "", func() {
					ui.TextInput(c.Key("input"), &e.team.Name).Label(t("settings_name")).Placeholder(t("form_name_hint")).FillWidth()
				})
				kit.Field(c, "description", t("teams_description"), "", func() {
					ui.TextInput(c.Key("input"), &e.team.Description).Label(t("teams_description")).Placeholder(t("teams_description")).FillWidth()
				})
				modelField(c, m, "lead", t("team_lead_model"), "team_follow_conversation", &e.team.Lead, &e.team.Effort, &e.team.Features)
				kit.Field(c, "instructions", t("team_lead_instructions"), "", func() {
					ui.TextArea(c.Key("input"), &e.team.Instructions).Label(t("team_lead_instructions")).FillWidth().MinHeight(80)
				})
			}
			members := e.team.Members
			modalList(c, "members", t("team_members"), roleCount(c, len(members)), t("team_members_empty"), len(members), func() {
				for i, id := range members {
					r := m.role(id)
					if r == nil {
						continue
					}
					sub := m.modelSummary(c, r.Model, "teams_follow_lead")
					if r.ReadOnly {
						sub += " · " + t("teams_read_only")
					}
					row := listRow{subtitle: sub, leading: func() { kit.RoleAvatar(c, r.ID, r.Avatar, 28) }}
					if !e.def {
						row.remove, row.removeLabel = func() { e.team.Members = slices.Delete(e.team.Members, i, i+1) }, t("team_member_remove")
					}
					row.row(c, id, r.Name)
				}
			}, func() {
				if e.def {
					return
				}
				kit.MenuButton(c.Key("add-role"), "team-add-role", func(b ui.Element) {
					kit.GhostLook(c, b.Label(t("team_add_role")), false).Height(24).PaddingX(6).Gap(4).Shrink(0).Children(func() {
						ui.Icon(c, icons.Plus).FontSize(14)
						ui.Text(c, t("team_add_role")).FontSize(13).SingleLine()
					})
				}, func(menu *Menu) {
					for _, r := range m.Roles {
						if slices.Contains(e.team.Members, r.ID) {
							continue
						}
						if menu.Item(r.Name).Description(r.Description).Chosen() {
							e.team.Members = append(e.team.Members, r.ID)
						}
					}
				})
			})
		})
		ui.Row(c.Key("footer")).Gap(8).Children(func() {
			if !e.def && e.team.ID != "" {
				kit.Secondary(c.Key("duplicate"), t("team_duplicate")).Disabled(e.pending || e.locked).OnClick(func() {
					x := e.team
					x.ID, x.Name, x.Members, x.Revision = "", t("team_copy_name", "name", x.Name), slices.Clone(x.Members), 0
					e.team = x
				})
				kit.Danger(c.Key("delete"), t("team_delete")).Disabled(e.pending || e.locked).OnClick(func() { m.teamRemoving = e.team.ID })
			}
			ui.Spacer(c)
			if e.def {
				kit.Secondary(c.Key("close"), t("close")).OnClick(func() { e.open = false })
				return
			}
			kit.Secondary(c.Key("cancel"), t("settings_cancel")).Disabled(e.pending || e.locked).OnClick(func() { e.open = false })
			action := t("settings_save")
			if e.team.ID == "" {
				action = t("team_create")
			}
			kit.PrimaryBusy(c.Key("ok"), action, false, e.pending).OnClick(func() {
				if m.Live && m.TeamActions != nil {
					e.pending = true
					token := new(bool)
					e.request = token
					m.TeamActions.SaveTeam(e.team, func(outcome kit.Outcome) {
						if e.request != token {
							return
						}
						e.pending = false
						e.locked = outcome == kit.Uncertain
						if outcome == kit.Done {
							e.open = false
						}
					})
					return
				}
				if problem := m.saveTeam(e.team); problem != "" {
					kit.Toast(c, kit.Failed, t(problem))
					return
				}
				kit.Toast(c, kit.Done, t("team_saved"))
				e.open = false
			})
		})
	}).Width(min(600, width(c)-48)).Label(title)
	if !e.open && (e.pending || e.locked) {
		e.open = true
	}
}

// saveTeam checks and stores a team: the i18n key of the problem, or "".
func (m *Model) saveTeam(x preview.Team) string {
	x.Name = strings.TrimSpace(x.Name)
	if x.Name == "" {
		return "team_invalid"
	}
	for _, y := range m.Teams {
		if y.ID != x.ID && strings.EqualFold(y.Name, x.Name) {
			return "team_name_taken"
		}
	}
	if tm := m.team(x.ID); tm != nil {
		*tm = x
		return ""
	}
	x.ID = fmt.Sprintf("team-%d", len(m.Teams)+1)
	m.Teams = append(m.Teams, x)
	return ""
}

// saveRole checks and stores a role: the i18n key of the problem, or "".
func (m *Model) saveRole(r preview.Role) string {
	r.Name, r.Key = strings.TrimSpace(r.Name), strings.TrimSpace(r.Key)
	valid := r.Key != "" && r.Name != ""
	for _, ch := range r.Key {
		if !(ch >= 'a' && ch <= 'z' || ch >= '0' && ch <= '9' || ch == '-' || ch == '_') {
			valid = false
		}
	}
	if !valid {
		return "teams_role_invalid"
	}
	for _, y := range m.Roles {
		if y.ID != r.ID && y.Key == r.Key {
			return "role_duplicate"
		}
	}
	if x := m.role(r.ID); x != nil {
		*x = r
		return ""
	}
	r.ID = r.Key
	m.Roles = append(m.Roles, r)
	return ""
}

// roleDialog is presets for a new role, name and identifier side by side,
// description, model, read-only and the instructions.
func roleDialog(c *ui.Context, m *Model) {
	e := &m.roleEdit
	if !e.open {
		return
	}
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	title := t("teams_role_edit")
	if e.role.ID == "" {
		title = t("role_new")
	}
	locked := e.pending || e.locked
	kit.Dialog(c.Key("role-editor"), &e.open, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600)
		ui.Scroll(c.Key("body")).Grow(1).Shrink(1).Gap(16).Children(func() {
			if e.role.ID == "" {
				kit.Field(c, "presets", t("role_presets"), "", func() {
					ui.Row(c).Wrap().Gap(6).Children(func() {
						for _, pr := range rolePresets {
							kit.Secondary(c.Key(pr[0]), t(pr[0])).Height(28).Disabled(e.pending || e.locked).OnClick(func() {
								e.role.Name, e.role.Description, e.role.Instructions = t(pr[0]), t(pr[1]), t(pr[2])
								if e.role.Key == "" {
									e.role.Key = strings.TrimPrefix(pr[0], "role_preset_")
								}
							})
						}
					})
				})
			}
			ui.Row(c.Key("pair")).Gap(16).AlignItems(ui.Start).Children(func() {
				ui.Column(c).Grow(1).Basis(0).Children(func() {
					kit.Field(c, "name", t("settings_name"), "", func() {
						ui.TextInput(c.Key("input"), &e.role.Name).Label(t("settings_name")).Placeholder(t("form_name_hint")).FillWidth().Disabled(locked)
					})
				})
				ui.Column(c).Grow(1).Basis(0).Children(func() {
					kit.Field(c, "key", t("role_id"), "", func() {
						ui.TextInput(c.Key("input"), &e.role.Key).Label(t("role_id")).Placeholder(t("teams_key_hint")).FillWidth().Disabled(locked)
					})
				})
			})
			avatarField(c, &e.role, e.pending || e.locked)
			kit.Field(c, "description", t("teams_description"), "", func() {
				ui.TextInput(c.Key("input"), &e.role.Description).Label(t("teams_description")).FillWidth().Disabled(locked)
			})
			ui.Column(c.Key("model-wrap")).Disabled(e.pending || e.locked).Children(func() {
				modelField(c, m, "model", t("teams_model"), "role_inherit", &e.role.Model, &e.role.Effort, &e.role.Features)
			})
			ui.Row(c.Key("read-only")).Gap(8).Children(func() {
				ui.Text(c, t("teams_read_only")).FontSize(14).Grow(1)
				ui.Switch(c.Key("switch"), &e.role.ReadOnly).Label(t("teams_read_only")).Disabled(locked)
			})
			kit.Field(c, "instructions", t("role_instructions"), "", func() {
				ui.TextArea(c.Key("input"), &e.role.Instructions).Label(t("role_instructions")).FillWidth().MinHeight(120).Disabled(locked)
			})
		})
		teamFooter(c, t("role_save"), e.pending, e.locked, func() { e.open = false }, func() {
			if e.pending {
				return
			}
			if m.Live && m.TeamActions != nil {
				e.pending = true
				token := new(bool)
				e.request = token
				m.TeamActions.SaveRole(e.role, func(outcome kit.Outcome) {
					if e.request != token {
						return
					}
					e.pending = false
					e.locked = outcome == kit.Uncertain
					if outcome == kit.Done {
						e.open = false
					}
				})
				return
			}
			if problem := m.saveRole(e.role); problem != "" {
				kit.Toast(c, kit.Failed, t(problem))
				return
			}
			kit.Toast(c, kit.Done, t("teams_role_saved"))
			e.open = false
		})
	}).Width(min(600, width(c)-48)).Label(title)
	if !e.open && (e.pending || e.locked) {
		e.open = true
	}
}

// teamFooter keeps uncertain drafts available for an explicit retry.
func teamFooter(c *ui.Context, action string, pending, locked bool, cancel, save func()) {
	ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Children(func() {
		kit.Secondary(c.Key("cancel"), i18n.T(c, "settings_cancel")).Disabled(pending || locked).OnClick(cancel)
		kit.PrimaryBusy(c.Key("ok"), action, false, pending).OnClick(save)
	})
}
