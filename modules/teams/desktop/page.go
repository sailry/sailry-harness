package desktop

import (
	"fmt"
	"maps"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func Page() settings.NativePage {
	return settings.NativePage{
		Section: settings.Teams,
		Group:   "settings_group_ai",
		Icon:    icons.Users,
		Order:   155,
		Render:  func(c *ui.Context, s *settings.State) { pageFor(s.Model).render(c) },
	}
}

type page struct {
	*settings.Model
	*pageState
}

type pageState struct {
	tab int
	*editors
}

type editors struct {
	teamEdit     teamEditor
	roleEdit     roleEditor
	teamRemoving string
	roleRemoving string
}

func pageFor(m *settings.Model) *page {
	state, ok := m.TeamPageState.(*pageState)
	if !ok {
		state = &pageState{editors: &editors{}}
		m.TeamPageState = state
	}
	return &page{Model: m, pageState: state}
}

// CloseDialogs detaches the previous Node's drafts from pending callbacks.
func (s *pageState) CloseDialogs() { s.editors = &editors{} }

var concurrencyMessages = settings.Messages{
	Failed: "teams_settings_failed", Conflict: "teams_settings_conflict", Uncertain: "teams_settings_unknown",
}

type rolePreset struct {
	key, label, description, prompt, modelHint string
}

var rolePresets = []rolePreset{
	{"review", "role_preset_review", "role_review_description", "role_prompt_review", "role_model_hint_review"},
	{"implement", "role_preset_implement", "role_implement_description", "role_prompt_implement", "role_model_hint_implement"},
	{"test", "role_preset_test", "role_test_description", "role_prompt_test", "role_model_hint_test"},
	{"plan", "role_preset_plan", "role_plan_description", "role_prompt_plan", "role_model_hint_plan"},
	{"frontend", "role_preset_frontend", "role_frontend_description", "role_prompt_frontend", "role_model_hint_frontend"},
	{"troubleshoot", "role_preset_troubleshoot", "role_troubleshoot_description", "role_prompt_troubleshoot", "role_model_hint_troubleshoot"},
}

// teamEditor edits a team: the Default team (default), a new one
// (team.ID == "") or a saved one.
type teamEditor struct {
	open            bool
	def             bool
	team            viewmodel.Team
	pending, locked bool
	request         *bool
}

// roleEditor edits a role, a new one when role.ID == "".
type roleEditor struct {
	open            bool
	role            viewmodel.Role
	preset          rolePreset
	readOnlyEdited  bool
	pending, locked bool
	request         *bool
}

func (e *roleEditor) modelHint() string {
	if e.preset.key != "" {
		return e.preset.modelHint
	}
	for _, preset := range rolePresets {
		if preset.key == e.role.Key {
			return preset.modelHint
		}
	}
	return ""
}

// modelChoices are the channels' models as "channel/model" with their
// labels, after none (the "" choice labeled follow).
func (m *page) modelChoices(c *ui.Context, follow string) (ids, names []string) {
	ids, names = []string{""}, []string{i18n.T(c, follow)}
	for _, ch := range m.ChannelsAt(m.ManagedNode) {
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

// modelFeatures are the features a model declares.
func (m *page) modelFeatures(id string) []viewmodel.ModelFeature {
	ch, model, _ := strings.Cut(id, "/")
	for _, x := range m.ChannelsAt(m.ManagedNode) {
		if x.ID != ch {
			continue
		}
		for _, y := range x.Models {
			if y.ID == model {
				return y.Features
			}
		}
	}
	return nil
}

// modelSummary is the model's label, or follow.
func (m *page) modelSummary(c *ui.Context, id, follow string) string {
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

func (m *page) role(id string) *viewmodel.Role {
	for i := range m.Roles {
		if m.Roles[i].ID == id {
			return &m.Roles[i]
		}
	}
	return nil
}

func (m *page) render(c *ui.Context) {
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	ui.Column(c.Key("teams")).Gap(16).Children(func() {
		kit.Card(c, func() {
			kit.WideRow(c, "concurrency", t("teams_concurrency"), t("teams_concurrency_description"), func() {
				shown := min(16, max(1, m.Concurrency.Draft))
				if ui.StepSlider(c.Key("slider"), &shown, 1, 16, 1).Label(t("teams_concurrency")).Grow(1).Disabled(m.ConcurrencyReadOnly || m.Concurrency.Pending || m.Live && m.TeamActions == nil).Changed() {
					m.Concurrency.Draft = shown
					if m.Live && m.TeamActions != nil {
						m.TeamActions.SaveConcurrency()
					} else {
						settings.Report(c, m.Concurrency.Commit(), concurrencyMessages)
					}
				}
				if m.Live && m.Concurrency.Pending {
					kit.Busy(c) // the limit is being saved
				}
				ui.Textf(c, "%.0f", m.Concurrency.Draft).FontSize(14).Width(24).TextAlign(ui.End)
			})
		})
		ui.Row(c.Key("bar")).Gap(12).Children(func() {
			kit.Segmented(c, "tabs", &m.tab, []string{t("settings_teams"), t("teams_roles")}, kit.SegmentedProps{}).Label(t("settings_teams"))
			ui.Spacer(c)
			label := t("team_new")
			if m.tab == 1 {
				label = t("role_new")
			}
			settings.IconButton(c, "new", icons.Plus, label, true, false).OnClick(func() {
				if m.tab == 0 {
					m.teamEdit = teamEditor{open: true}
				} else {
					m.roleEdit = roleEditor{open: true}
				}
			})
		})
		if m.tab == 0 {
			teamRows(c, m)
		} else {
			roleRows(c, m)
		}
	})
	teamDialog(c, m)
	roleDialog(c, m)
	drafts := m.editors
	if tm := m.team(drafts.teamRemoving); tm != nil {
		open := true
		if settings.Confirm(c, "delete-team", &open, t("team_delete"), t("team_delete_prompt", "name", tm.Name), t("settings_delete")) {
			if m.Live && m.TeamActions != nil {
				// The editor it was deleted from closes; one opened since for
				// another team keeps its draft.
				id := tm.ID
				m.TeamActions.RemoveTeam(*tm, func(outcome kit.Outcome) {
					if outcome != kit.Done {
						return
					}
					if drafts.teamEdit.team.ID == id {
						drafts.teamEdit.open = false
					}
					if drafts.teamRemoving == id {
						drafts.teamRemoving = ""
					}
				})
			} else {
				m.Teams = slices.DeleteFunc(m.Teams, func(x viewmodel.Team) bool { return x.ID == tm.ID })
				drafts.teamEdit.open = false
				kit.Toast(c, kit.Done, t("team_deleted"))
			}
		}
		if !open {
			drafts.teamRemoving = ""
		}
	}
	if r := m.role(drafts.roleRemoving); r != nil {
		open := true
		if settings.Confirm(c, "delete-role", &open, t("settings_delete"), t("teams_role_delete_prompt", "name", r.Name), t("settings_delete")) {
			if m.Live && m.TeamActions != nil {
				id := r.ID
				m.TeamActions.RemoveRole(*r, func(outcome kit.Outcome) {
					if outcome == kit.Done && drafts.roleRemoving == id {
						drafts.roleRemoving = ""
					}
				})
			} else {
				m.Roles = slices.DeleteFunc(m.Roles, func(x viewmodel.Role) bool { return x.ID == r.ID })
				for i := range m.Teams {
					m.Teams[i].Members = slices.DeleteFunc(m.Teams[i].Members, func(id string) bool { return id == r.ID })
				}
				kit.Toast(c, kit.Done, t("teams_role_deleted"))
			}
		}
		if !open {
			drafts.roleRemoving = ""
		}
	}
}

func (m *page) team(id string) *viewmodel.Team {
	for i := range m.Teams {
		if id != "" && m.Teams[i].ID == id {
			return &m.Teams[i]
		}
	}
	return nil
}

// teamRows is the Default team, then the saved teams, each with its members'
// marks; clicking opens the editor.
func teamRows(c *ui.Context, m *page) {
	p := theme.Of(c)
	row := func(key, name, desc, meta, avatar string, builtin bool, members []string, open func()) {
		kit.Bleed(c, "row:"+key, func() {
			b := ui.ButtonBase(c.Key(key)).Label(name).Padding(12, 16).Margin(0, -16).Gap(12).Justify(ui.Start).Cursor(ui.CursorPointer).OnClick(open)
			if b.Hovered() {
				b.Background(p.Hover)
			}
			b.Children(func() {
				kit.MediaRow(c, name, desc, func() { kit.TeamAvatar(c, key, avatar, kit.TileSize) }, func() {
					if builtin {
						kit.Tag(c, i18n.T(c, "team_builtin"))
					}
				})
				ui.Row(c).Shrink(0).Gap(4).Children(func() {
					for i, id := range members[:min(5, len(members))] {
						if r := m.role(id); r != nil {
							kit.RoleAvatar(c.Key(i), r.ID, r.Avatar, 20)
						}
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
		row("default", i18n.T(c, "team_default"), i18n.T(c, "team_default_description"), roleCount(c, len(all)), "", true, all, func() {
			m.teamEdit = teamEditor{open: true, def: true, team: viewmodel.Team{Name: i18n.T(c, "team_default"), Members: all}}
		})
		for i := range m.Teams {
			tm := &m.Teams[i]
			meta := roleCount(c, len(tm.Members)) + " · " + m.modelSummary(c, tm.Lead, "team_follow_conversation")
			if tm.ConfigurationReadOnly {
				meta += " · " + i18n.T(c, "configuration_follow_from_source")
			}
			row(tm.ID, tm.Name, tm.Description, meta, tm.Avatar, false, tm.Members, func() {
				if tm.ConfigurationReadOnly {
					return
				}
				x := *tm
				x.Members = slices.Clone(tm.Members)
				m.teamEdit = teamEditor{open: true, team: x}
			})
		}
	})
}

// roleRows is each role with its model, and its edit and delete actions.
func roleRows(c *ui.Context, m *page) {
	if len(m.Roles) == 0 {
		kit.Result(c, kit.ResultProps{Icon: icons.Bot, Title: i18n.T(c, "teams_roles_empty"), Card: true})
		return
	}
	kit.Card(c, func() {
		for i := range m.Roles {
			r := &m.Roles[i]
			desc := m.modelSummary(c, r.Model, "role_inherit")
			if r.ConfigurationReadOnly {
				desc += " · " + i18n.T(c, "configuration_follow_from_source")
			}
			if r.ReadOnly {
				desc += " · " + i18n.T(c, "teams_read_only")
			}
			kit.Entry(c, r.ID, func() {
				kit.MediaRow(c, r.Name, desc, func() { kit.RoleAvatar(c, r.ID, r.Avatar, kit.TileSize) }, nil)
			}, nil, func() {
				kit.Action(c.Key("edit"), icons.Pencil, i18n.T(c, "settings_edit"), false).Disabled(r.ConfigurationReadOnly).OnClick(func() { m.roleEdit = roleEditor{open: true, role: *r} })
				kit.Action(c.Key("delete"), icons.Trash, i18n.T(c, "settings_delete"), false).Disabled(r.ConfigurationReadOnly).OnClick(func() { m.roleRemoving = r.ID })
			})
		}
	})
}

// modelField is the model select of "" (labeled follow) and the channels'
// models, its efforts after Auto beside them in its popover, with one
// select per feature the model declares after it. Another model starts
// from Auto. A feature follows (Auto, features nil), is off (absent from
// the map) or takes one of its options.
func modelField(c *ui.Context, m *page, key, label, follow string, model, effort *string, features *map[string]string) {
	kit.Field(c, key, label, "", func() {
		ui.Row(c).Gap(8).Wrap().Children(func() {
			kit.ModelSelect(c.Key("model"), kit.ModelSelectProps{Label: label, None: i18n.T(c, follow), Models: m.ModelOptions(m.ManagedNode),
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
				if settings.Pick(c, "feature-"+f.ID, f.Name, &at, shown, false).Width(140).Changed() {
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
func teamDialog(c *ui.Context, m *page) {
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
				avatarField(c, teamAvatars(e.team.ID), &e.team.Avatar, e.pending || e.locked)
				kit.Field(c, "description", t("teams_description"), "", func() {
					ui.TextInput(c.Key("input"), &e.team.Description).Label(t("teams_description")).Placeholder(t("teams_description")).FillWidth()
				})
				modelField(c, m, "lead", t("team_lead_model"), "team_follow_conversation", &e.team.Lead, &e.team.Effort, &e.team.Features)
				kit.Field(c, "instructions", t("team_lead_instructions"), "", func() {
					ui.TextArea(c.Key("input"), &e.team.Instructions).Label(t("team_lead_instructions")).FillWidth().MinHeight(80)
				})
			}
			members := e.team.Members
			settings.ModalList(c, "members", t("team_members"), roleCount(c, len(members)), t("team_members_empty"), len(members), func() {
				for i, id := range members {
					r := m.role(id)
					if r == nil {
						continue
					}
					sub := m.modelSummary(c, r.Model, "teams_follow_lead")
					if r.ReadOnly {
						sub += " · " + t("teams_read_only")
					}
					row := settings.ListRow{Subtitle: sub, Leading: func() { kit.RoleAvatar(c, r.ID, r.Avatar, 28) }}
					if !e.def {
						row.Remove, row.RemoveLabel = func() { e.team.Members = slices.Delete(e.team.Members, i, i+1) }, t("team_member_remove")
					}
					row.Render(c, id, r.Name)
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
				}, func(menu *kit.Menu) {
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
	}).Width(settings.DialogWidth(c, 600)).Label(title)
	if !e.open && (e.pending || e.locked) {
		e.open = true
	}
}

// saveTeam checks and stores a team: the i18n key of the problem, or "".
func (m *page) saveTeam(x viewmodel.Team) string {
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
func (m *page) saveRole(r viewmodel.Role) string {
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
func roleDialog(c *ui.Context, m *page) {
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
					ui.Row(c).FillWidth().Wrap().Gap(6).Children(func() {
						for _, pr := range rolePresets {
							kit.Secondary(c.Key(pr.key), t(pr.label)).Height(28).Disabled(locked).OnClick(func() {
								e.role.Name, e.role.Description, e.role.Instructions = t(pr.label), t(pr.description), t(pr.prompt)
								if e.role.Key == "" || e.role.Key == e.preset.key {
									e.role.Key = pr.key
								}
								if !e.readOnlyEdited {
									e.role.ReadOnly = pr.key == "plan"
								}
								e.preset = pr
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
			avatarField(c, roleAvatars(e.role.ID), &e.role.Avatar, e.pending || e.locked)
			kit.Field(c, "description", t("teams_description"), "", func() {
				ui.TextInput(c.Key("input"), &e.role.Description).Label(t("teams_description")).FillWidth().Disabled(locked)
			})
			ui.Column(c.Key("model-wrap")).Gap(8).Disabled(locked).Children(func() {
				modelField(c, m, "model", t("teams_model"), "role_inherit", &e.role.Model, &e.role.Effort, &e.role.Features)
				if hint := e.modelHint(); hint != "" {
					ui.Text(c, t(hint)).FontSize(12).TextColor(theme.Of(c).MutedForeground)
				}
			})
			ui.Row(c.Key("read-only")).Gap(8).Children(func() {
				ui.Text(c, t("teams_read_only")).FontSize(14).Grow(1)
				if ui.Switch(c.Key("switch"), &e.role.ReadOnly).Label(t("teams_read_only")).Disabled(locked).Changed() {
					e.readOnlyEdited = true
				}
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
	}).Width(settings.DialogWidth(c, 600)).Label(title)
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
