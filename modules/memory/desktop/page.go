package desktop

import (
	"fmt"
	"slices"
	"strconv"
	"strings"
	"unicode"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func Page() settings.NativePage {
	return settings.NativePage{
		Section: settings.Memory,
		Group:   "settings_group_ai",
		Icon:    icons.Brain,
		Order:   500,
		Render:  render,
	}
}

var (
	memoryViews   = []string{"active", "archived", "review"}
	memoryKinds   = []string{"user", "feedback", "project", "reference"}
	memoryBudgets = []uint32{2, 4, 8, 16, 32, 64}
	memoryAges    = []uint32{30, 90, 180, 365}
)

type pageState struct {
	generation     uint64
	syncQuery      bool
	editorOpen     bool
	editingID      string
	editingRev     uint64
	editingProject string
	title          string
	kind           string
	body           string
	archived       bool
	sources        []settings.MemoryMergeSource
	editorLoading  bool
	editorSaving   bool
	editorRetry    bool
	editorError    string
	deleteOpen     bool
	deleteTarget   settings.MemoryEntryItem
	deleteSaving   bool
	deleteRetry    bool
	lastQuery      string
}

func (st *pageState) CloseDialogs() {
	st.generation++
	st.syncQuery = true
	st.editorOpen = false
	st.editorLoading = false
	st.editorSaving = false
	st.editorRetry = false
	st.editorError = ""
	st.sources = nil
	st.deleteOpen = false
	st.deleteSaving = false
	st.deleteRetry = false
}

func render(c *ui.Context, s *settings.State) {
	st := s.NativePageState(settings.Memory, func() any { return &pageState{} }).(*pageState)
	s.MemoryPageState = st
	m := &s.MemorySettings
	act := s.MemoryActions
	t := func(key string) string { return i18n.T(c, key) }

	if m.Status != "" {
		kit.Result(c, kit.ResultProps{Title: t(m.Status), Card: true})
		return
	}

	if m.View == "" {
		m.View = "active"
	}
	if m.Scope == "" {
		m.Scope = "all"
	}

	renderConfiguration(c, s, m, act)
	renderToolbar(c, s, st, m, act)
	renderCatalog(c, s, st, m, act)
	renderEditor(c, s, st, m, act)
	renderDelete(c, s, st, m, act)
}

func renderConfiguration(c *ui.Context, s *settings.State, m *settings.MemorySettingsState, act *settings.MemoryActions) {
	t := func(key string) string { return i18n.T(c, key) }
	kit.Group(c, "memory_configuration", t("memory_configuration"), "", nil, func() {
		if !m.Loaded && m.Loading {
			kit.Card(c, func() {
				ui.Row(c).Height(96).Center().Children(func() { kit.Busy(c) })
			})
			return
		}
		cfg := &m.Config
		disabled := !m.Connected || !m.Loaded || cfg.Pending || m.ConfigRetry || m.ConfigError != ""
		saveConfig := func() {
			if s.Live && act != nil && act.SaveConfig != nil {
				act.SaveConfig()
			} else {
				cfg.Commit()
			}
		}
		kit.Card(c, func() {
			kit.Row(c, "enabled", t("memory_enabled"), "", func() {
				if ui.Switch(c.Key("switch"), &cfg.Draft.Enabled).Label(t("memory_enabled")).Disabled(disabled).Changed() {
					saveConfig()
				}
			})
			kit.Row(c, "auto_write", t("memory_auto_write"), t("memory_auto_write_description"), func() {
				if ui.Switch(c.Key("switch"), &cfg.Draft.AutoWrite).Label(t("memory_auto_write")).Disabled(disabled || !cfg.Draft.Enabled).Changed() {
					saveConfig()
				}
			})
			budgetStr := strconv.FormatUint(uint64(max(cfg.Draft.ContextBytes/1024, 2)), 10)
			budgetOpts := make([]kit.SelectOption, 0, len(memoryBudgets))
			for _, kb := range memoryBudgets {
				val := strconv.FormatUint(uint64(kb), 10)
				budgetOpts = append(budgetOpts, kit.SelectOption{Value: val, Label: fmt.Sprintf("%d KiB", kb)})
			}
			kit.WideRow(c, "budget", t("memory_context_budget"), t("memory_budget_description"), func() {
				el, changed := kit.SelectField(c, "memory-budget", &budgetStr, kit.SelectProps{
					Label:    t("memory_context_budget"),
					Options:  budgetOpts,
					Disabled: disabled,
				})
				el.Grow(1).Shrink(1).MinWidth(0)
				if changed {
					if kb, err := strconv.ParseUint(budgetStr, 10, 32); err == nil {
						cfg.Draft.ContextBytes = uint32(kb) * 1024
						saveConfig()
					}
				}
			})
			ageStr := strconv.FormatUint(uint64(max(cfg.Draft.ReviewAfterDays, 30)), 10)
			ageOpts := make([]kit.SelectOption, 0, len(memoryAges))
			for _, days := range memoryAges {
				val := strconv.FormatUint(uint64(days), 10)
				ageOpts = append(ageOpts, kit.SelectOption{Value: val, Label: val})
			}
			kit.WideRow(c, "review_age", t("memory_review_age"), "", func() {
				el, changed := kit.SelectField(c, "memory-review-age", &ageStr, kit.SelectProps{
					Label:    t("memory_review_age"),
					Options:  ageOpts,
					Disabled: disabled,
				})
				el.Grow(1).Shrink(1).MinWidth(0)
				if changed {
					if days, err := strconv.ParseUint(ageStr, 10, 32); err == nil {
						cfg.Draft.ReviewAfterDays = uint32(days)
						saveConfig()
					}
				}
			})
		})
		if m.ConfigError != "" {
			ui.Row(c.Key("config-recovery")).Gap(8).Children(func() {
				if m.ConfigError != "memory_settings_conflict" {
					kit.SecondaryBusy(c.Key("retry"), t("memory_retry"), cfg.Pending).Disabled(!m.Connected).OnClick(func() {
						saveConfig()
					})
				}
				kit.Secondary(c.Key("reload"), t("memory_settings_reload")).Disabled(cfg.Pending).OnClick(func() {
					if act != nil && act.ReloadConfig != nil {
						act.ReloadConfig()
					} else {
						cfg.Draft = cfg.Saved
						m.ConfigError = ""
					}
				})
			})
		}
	})
}

func renderToolbar(c *ui.Context, s *settings.State, st *pageState, m *settings.MemorySettingsState, act *settings.MemoryActions) {
	t := func(key string) string { return i18n.T(c, key) }
	busy := !m.Connected || m.Loading
	viewIdx := max(slices.Index(memoryViews, m.View), 0)
	labels := []string{t("memory_active"), t("memory_archived"), t("memory_review")}

	ui.Row(c.Key("memory-toolbar")).FillWidth().Wrap().AlignItems(ui.Center).Gap(12).Children(func() {
		if kit.Segmented(c, "memory-views", &viewIdx, labels, kit.SegmentedProps{}).Label(t("settings_memory")).Changed() {
			m.View = memoryViews[viewIdx]
			if act != nil && act.Refresh != nil {
				act.Refresh()
			}
		}
		ui.Row(c.Key("filters")).Margin(0, 0, 0, ui.Auto).AlignItems(ui.Center).Gap(8).Wrap().Children(func() {
			if st.syncQuery {
				st.syncQuery = false
				st.lastQuery = m.Query
			}
			ui.SearchField(c.Key("memory-search"), &m.Query).Label(t("memory_search")).Placeholder(t("memory_search")).Width(180)
			if m.Query != st.lastQuery {
				st.lastQuery = m.Query
				if act != nil && act.Refresh != nil {
					act.Refresh()
				}
			}
			scopeOpts := make([]kit.SelectOption, 0, len(m.Projects)+2)
			scopeOpts = append(scopeOpts,
				kit.SelectOption{Value: "all", Label: t("memory_all")},
				kit.SelectOption{Value: "global", Label: t("memory_global")},
			)
			for _, project := range m.Projects {
				scopeOpts = append(scopeOpts, kit.SelectOption{Value: project.ID, Label: project.Name})
			}
			scope := m.Scope
			el, changed := kit.SelectField(c, "memory-scope", &scope, kit.SelectProps{
				Label:       t("memory_scope"),
				Placeholder: t("memory_all"),
				Options:     scopeOpts,
			})
			el.Width(180)
			if changed {
				m.Scope = scope
				if act != nil && act.Refresh != nil {
					act.Refresh()
				}
			}
			kit.Primary(c.Key("memory-create"), t("memory_create"), busy || st.editorSaving || st.deleteSaving).
				Children(func() { ui.Text(c, t("memory_create")).FontSize(14).SingleLine() }).
				OnClick(func() {
					project := ""
					if m.Scope != "all" && m.Scope != "global" {
						project = m.Scope
					}
					if act != nil && act.ForgetPending != nil {
						act.ForgetPending()
					}
					st.CloseDialogs()
					st.editorOpen = true
					st.editingID = ""
					st.editingRev = 0
					st.editingProject = project
					st.title = ""
					st.body = ""
					st.archived = false
					st.kind = "user"
				})
		})
	})
}

func renderCatalog(c *ui.Context, s *settings.State, st *pageState, m *settings.MemorySettingsState, act *settings.MemoryActions) {
	t := func(key string) string { return i18n.T(c, key) }
	p := theme.Of(c)
	busy := !m.Connected || m.Loading || st.editorSaving || st.deleteSaving

	projectLabel := func(id string) string {
		if id == "" {
			return t("memory_global")
		}
		for _, proj := range m.Projects {
			if proj.ID == id {
				return proj.Name
			}
		}
		return t("memory_project_unavailable")
	}

	kit.Group(c, "memory_catalog", "", "", nil, func() {
		kit.Card(c, func() {
			if !m.Loaded {
				if m.Error != "" && !m.Loading {
					ui.Row(c).Padding(16, 0).Children(func() {
						kit.Secondary(c.Key("retry"), t("memory_retry")).Disabled(!m.Connected).OnClick(func() {
							if act != nil && act.Refresh != nil {
								act.Refresh()
							}
						})
					})
				} else {
					ui.Row(c).Height(96).Center().Children(func() { kit.Busy(c) })
				}
				return
			}
			if len(m.Entries) == 0 {
				kit.Result(c, kit.ResultProps{Icon: icons.Inbox, Title: t("memory_empty")})
				return
			}
			for _, entry := range m.Entries {
				entry := entry
				kit.Entry(c, entry.ID, func() {
					ui.Column(c).MinWidth(0).Gap(4).Children(func() {
						ui.Text(c, entry.Title).FontSize(14).FontWeight(500).SingleLine().TextColor(p.SurfaceForeground)
						if entry.NeedsReview {
							reviewKey := "memory_needs_review"
							if entry.DuplicateOf != "" {
								reviewKey = "memory_possible_duplicate"
							}
							ui.Text(c, t(reviewKey)).FontSize(12).TextColor(p.MutedForeground)
						}
						ui.Row(c).AlignItems(ui.Center).Gap(8).Children(func() {
							ui.Text(c, t("memory_"+entry.Kind)).FontSize(12).TextColor(p.MutedForeground)
							kit.Tag(c, projectLabel(entry.Project))
						})
					})
				}, nil, func() {
					kit.Action(c.Key("edit"), icons.Pencil, t("settings_edit"), busy).OnClick(func() {
						if act != nil && act.ForgetPending != nil {
							act.ForgetPending()
						}
						openMemoryEditor(st, entry, s.Live, act)
					})
					kit.Action(c.Key("delete"), icons.Trash, t("settings_delete"), busy).OnClick(func() {
						if act != nil && act.ForgetPending != nil {
							act.ForgetPending()
						}
						st.CloseDialogs()
						st.deleteOpen = true
						st.deleteTarget = entry
					})
				})
			}
		})
	})
}

func openMemoryEditor(st *pageState, entry settings.MemoryEntryItem, live bool, act *settings.MemoryActions) {
	st.CloseDialogs()
	st.editorOpen = true
	st.editingID = entry.ID
	st.editingRev = entry.Revision
	st.editingProject = entry.Project
	st.title = entry.Title
	st.kind = entry.Kind
	st.archived = entry.Archived
	st.body = ""
	if live && act != nil && act.LoadEntry != nil {
		st.editorLoading = true
		id := entry.ID
		gen := st.generation
		act.LoadEntry(id, func(loaded settings.MemoryEntryItem, body string, errKey string) {
			if st.generation != gen || !st.editorOpen || st.editingID != id {
				return
			}
			st.editorLoading = false
			if errKey != "" {
				st.editorError = errKey
				return
			}
			st.editingRev = loaded.Revision
			st.editingProject = loaded.Project
			st.title = loaded.Title
			st.kind = loaded.Kind
			st.archived = loaded.Archived
			st.body = body
			st.editorError = ""
		})
	}
}

func renderEditor(c *ui.Context, s *settings.State, st *pageState, m *settings.MemorySettingsState, act *settings.MemoryActions) {
	t := func(key string) string { return i18n.T(c, key) }
	p := theme.Of(c)
	open := st.editorOpen
	loadFailed := st.editorError != "" && st.editingRev > 0 && st.body == ""
	locked := st.editorLoading || st.editorSaving || st.editorRetry || loadFailed

	dialogTitle := t("memory_create")
	if st.editingRev > 0 {
		dialogTitle = t("memory_editor")
	}

	kit.Dialog(c.Key("memory-editor-dialog"), &open, func() {
		kit.DialogHeader(c, dialogTitle)
		if st.editorLoading {
			ui.Row(c).Height(120).Center().Children(func() { kit.Busy(c) })
		} else {
			if loadFailed {
				kit.Alert(c, "editor-load-error", kit.Negative, t(st.editorError), nil)
			}
			kit.Field(c, "title", t("memory_title"), "", func() {
				ui.TextInput(c.Key("input"), &st.title).Label(t("memory_title")).Placeholder(t("memory_title_hint")).Disabled(locked).FillWidth()
			})
			kindIdx := max(slices.Index(memoryKinds, st.kind), 0)
			kindLabels := []string{t("memory_user"), t("memory_feedback"), t("memory_project"), t("memory_reference")}
			kit.Field(c, "kind", t("memory_kind"), "", func() {
				if kit.Segmented(c, "kind-tabs", &kindIdx, kindLabels, kit.SegmentedProps{Block: true, Disabled: locked}).Label(t("memory_kind")).Changed() {
					st.kind = memoryKinds[kindIdx]
				}
			})
			kit.Field(c, "body", t("memory_body"), "", func() {
				ui.TextArea(c.Key("textarea"), &st.body).Label(t("memory_body")).Placeholder(t("memory_body_hint")).Disabled(locked).FillWidth().MinHeight(140)
			})
			if st.editingRev > 0 {
				ui.Row(c.Key("archive-row")).AlignItems(ui.Center).Gap(8).Children(func() {
					ui.Switch(c.Key("switch"), &st.archived).Label(t("memory_archived")).Disabled(locked || len(st.sources) > 0)
					ui.Text(c, t("memory_archived")).FontSize(14).TextColor(p.PopoverForeground)
				})
				var candidates []settings.MemoryEntryItem
				for _, cand := range m.AllActive {
					if !cand.Archived && cand.Project == st.editingProject && cand.ID != st.editingID {
						candidates = append(candidates, cand)
					}
				}
				if !st.archived && len(candidates) > 0 {
					kit.MenuButton(c, "memory-merge-source", func(b ui.Element) {
						kit.SecondaryLook(c, b.Label(t("memory_merge_source")).Disabled(locked || len(st.sources) > 0)).Children(func() {
							ui.Text(c, t("memory_merge_source")).FontSize(14).SingleLine()
							ui.Icon(c, icons.ChevronDown).FontSize(12)
						})
					}, func(menu *kit.Menu) {
						for _, cand := range candidates {
							cand := cand
							if menu.Item(cand.Title).Chosen() && act != nil && act.LoadEntry != nil {
								st.editorLoading = true
								gen := st.generation
								act.LoadEntry(cand.ID, func(loaded settings.MemoryEntryItem, body string, errKey string) {
									if st.generation != gen || !st.editorOpen {
										return
									}
									st.editorLoading = false
									if errKey != "" || loaded.Project != st.editingProject || loaded.Archived {
										return
									}
									st.sources = []settings.MemoryMergeSource{{ID: loaded.ID, Revision: loaded.Revision}}
									if st.body != "" {
										st.body += "\n\n"
									}
									st.body += body
								})
							}
						}
					}, kit.MenuOptions{MaxHeight: 280})
				}
				if len(st.sources) > 0 {
					ui.Text(c, t("memory_merge_description")).FontSize(13).TextColor(p.MutedForeground)
				}
			}
		}

		kit.DialogFooter(c, func() {
			kit.Secondary(c.Key("cancel"), t("settings_cancel")).Disabled(st.editorSaving).OnClick(func() {
				if st.editorRetry && act != nil && act.ForgetPending != nil {
					act.ForgetPending()
				}
				st.CloseDialogs()
			})
			if loadFailed {
				kit.Secondary(c.Key("reload"), t("memory_retry")).OnClick(func() {
					openMemoryEditor(st, settings.MemoryEntryItem{
						ID: st.editingID, Revision: st.editingRev, Project: st.editingProject,
						Title: st.title, Kind: st.kind, Archived: st.archived,
					}, s.Live, act)
				})
			}
			saveLabel := t("settings_save")
			if st.editorRetry {
				saveLabel = t("memory_retry")
			} else if len(st.sources) > 0 {
				saveLabel = t("memory_merge")
			}
			kit.PrimaryBusy(c.Key("save"), saveLabel, st.editorLoading || loadFailed || !m.Connected, st.editorSaving).OnClick(func() {
				title := strings.TrimSpace(st.title)
				if !validMemoryInput(title, st.body) {
					kit.Toast(c, kit.Failed, t("memory_invalid"))
					return
				}
				draft := settings.MemorySaveDraft{
					ID:               st.editingID,
					ExpectedRevision: st.editingRev,
					Project:          st.editingProject,
					Title:            title,
					Kind:             st.kind,
					Body:             st.body,
					Archived:         st.archived,
					MergeSources:     slices.Clone(st.sources),
				}
				if !s.Live || act == nil || act.SaveEntry == nil {
					st.CloseDialogs()
					return
				}
				st.editorSaving = true
				gen := st.generation
				act.SaveEntry(draft, func(ok bool, retry bool, errKey string) {
					if st.generation != gen {
						return
					}
					st.editorSaving = false
					st.editorRetry = retry
					st.editorError = errKey
					if ok {
						st.CloseDialogs()
					}
				})
			})
		})
	}).Width(settings.DialogWidth(c, 580+2*kit.DialogInset)).Label(dialogTitle)
	if !open && st.editorOpen && !st.editorSaving {
		if st.editorRetry && act != nil && act.ForgetPending != nil {
			act.ForgetPending()
		}
		st.CloseDialogs()
	}
}

func renderDelete(c *ui.Context, s *settings.State, st *pageState, m *settings.MemorySettingsState, act *settings.MemoryActions) {
	t := func(key string) string { return i18n.T(c, key) }
	p := theme.Of(c)
	open := st.deleteOpen
	title := t("memory_delete")
	kit.Dialog(c.Key("memory-delete-dialog"), &open, func() {
		kit.DialogHeader(c, title)
		ui.Text(c, st.deleteTarget.Title).FontSize(14).FontWeight(500).TextColor(p.PopoverForeground)
		ui.Text(c, t("memory_delete_confirm")).FontSize(13).TextColor(p.MutedForeground)
		kit.DialogFooter(c, func() {
			kit.Secondary(c.Key("cancel"), t("settings_cancel")).Disabled(st.deleteSaving).OnClick(func() {
				if st.deleteRetry && act != nil && act.ForgetPending != nil {
					act.ForgetPending()
				}
				st.CloseDialogs()
			})
			delLabel := t("settings_delete")
			if st.deleteRetry {
				delLabel = t("memory_retry")
			}
			kit.DangerBusy(c.Key("confirm"), delLabel, st.deleteSaving).Disabled(!m.Connected).OnClick(func() {
				if !s.Live || act == nil || act.DeleteEntry == nil {
					st.CloseDialogs()
					return
				}
				st.deleteSaving = true
				gen := st.generation
				act.DeleteEntry(st.deleteTarget.ID, st.deleteTarget.Revision, func(ok bool, retry bool, _ string) {
					if st.generation != gen {
						return
					}
					st.deleteSaving = false
					st.deleteRetry = retry
					if ok {
						st.CloseDialogs()
					}
				})
			})
		})
	}).Width(settings.DialogWidth(c, 420+2*kit.DialogInset)).Label(title)
	if !open && st.deleteOpen && !st.deleteSaving {
		if st.deleteRetry && act != nil && act.ForgetPending != nil {
			act.ForgetPending()
		}
		st.CloseDialogs()
	}
}

func validMemoryInput(title, body string) bool {
	if title == "" || len(title) > 160 {
		return false
	}
	for _, r := range title {
		if unicode.Is(unicode.Cc, r) {
			return false
		}
	}
	return strings.TrimSpace(body) != "" && len(body) <= 8192 && !strings.ContainsRune(body, 0)
}
