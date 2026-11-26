package scheduledtasksdesktop

import (
	"fmt"
	"slices"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const editorDialogWidth = 560

func (ctrl *controller) render(c *ui.Context) {
	ctrl.flushToasts(c)
	t := func(key string, args ...any) string { return i18n.T(c, key, args...) }

	ctrl.mu.Lock()
	loaded := ctrl.loaded
	loading := ctrl.loading
	busy := ctrl.busy
	pending := ctrl.pending
	projFilter := ctrl.projFilter
	items := slices.Clone(ctrl.items)
	nodes := slices.Clone(ctrl.nodes)
	projects := slices.Clone(ctrl.projects)
	var panelTaskID, panelTab string
	if ctrl.panel != nil {
		panelTaskID, panelTab = ctrl.panel.taskID, ctrl.panel.tab
	}
	hasEditor := ctrl.editor != nil
	hasDelete := ctrl.deleting != nil
	ctrl.mu.Unlock()

	nowMs := ctrl.now().UnixMilli()
	multiHost := len(nodes) > 1
	disabled := busy || pending != nil

	filtered := make([]TaskItem, 0, len(items))
	for _, it := range items {
		switch projFilter {
		case "all":
			filtered = append(filtered, it)
		case "none":
			if it.Task.Project == nil {
				filtered = append(filtered, it)
			}
		default:
			if it.Task.Project != nil && it.Task.Project.String() == projFilter {
				filtered = append(filtered, it)
			}
		}
	}

	kit.Page(c, "scheduled-tasks-page", t("scheduled_tasks_title"), t("scheduled_tasks_description"), func() {
		// Toolbar: Project filter + New task button.
		ui.Row(c.Key("toolbar")).FillWidth().Wrap().AlignItems(ui.Center).Justify(ui.SpaceBetween).Gap(12).Children(func() {
			filterOpts := ctrl.projectFilterOptions(projects, items, t)
			selectedFilter := projFilter
			el, changed := kit.SelectField(c.Key("project-filter"), "tasks-project-filter", &selectedFilter, kit.SelectProps{
				Label:       t("scheduled_tasks_project"),
				Placeholder: t("scheduled_tasks_all_projects"),
				Options:     filterOpts,
			})
			el.Width(200).MaxWidthPercent(100)
			if changed {
				ctrl.mu.Lock()
				ctrl.projFilter = selectedFilter
				ctrl.mu.Unlock()
			}
			kit.Primary(c.Key("create"), t("scheduled_tasks_create"), disabled).Children(func() {
				ui.Text(c, t("scheduled_tasks_create")).FontSize(14).SingleLine()
			}).OnClick(func() {
				ctrl.openEditor(nil)
			})
		})

		// Uncertain durable mutation recovery banner.
		if !hasEditor && !hasDelete && pending != nil {
			msgKey := pending.errorKey
			if msgKey == "" {
				msgKey = "scheduled_tasks_unknown"
			}
			kit.Alert(c, "pending-banner", kit.Caution, t(msgKey), func() {
				ui.Row(c).Shrink(0).Gap(8).Children(func() {
					kit.SecondaryBusy(c.Key("retry"), t("scheduled_tasks_retry"), busy).OnClick(ctrl.retryPending)
					kit.Secondary(c.Key("cancel"), t("scheduled_tasks_cancel")).Disabled(busy).OnClick(ctrl.dismissPending)
				})
			})
		}

		// Task cards list.
		if !loaded && loading {
			kit.Card(c, func() {
				ui.Row(c).FillWidth().Height(96).Center().Children(func() {
					kit.Loader(c, 24)
				})
			})
			return
		}
		if len(filtered) == 0 {
			kit.Result(c, kit.ResultProps{
				Icon:  icons.Calendar,
				Title: t("scheduled_tasks_empty"),
				Card:  true,
			})
			return
		}
		ui.Column(c.Key("task-list")).FillWidth().Gap(10).Children(func() {
			for _, item := range filtered {
				ctrl.renderTaskCard(c, item, nowMs, disabled, multiHost, panelTaskID, panelTab, t)
			}
		})
	})

	ctrl.renderEditorDialog(c, t)
	ctrl.renderDeleteConfirm(c, t)
}

func (ctrl *controller) projectFilterOptions(projects []projectOption, items []TaskItem, t Translate) []kit.SelectOption {
	opts := []kit.SelectOption{
		{Value: "all", Label: t("scheduled_tasks_all_projects")},
		{Value: "none", Label: t("scheduled_tasks_no_project")},
	}
	seen := map[string]bool{}
	for _, pr := range projects {
		id := pr.Project.ID.String()
		if seen[id] {
			continue
		}
		seen[id] = true
		opts = append(opts, kit.SelectOption{Value: id, Label: pr.Project.Name})
	}
	for _, it := range items {
		if it.Task.Project == nil {
			continue
		}
		id := it.Task.Project.String()
		if seen[id] {
			continue
		}
		seen[id] = true
		opts = append(opts, kit.SelectOption{Value: id, Label: t("scheduled_tasks_unavailable")})
	}
	return opts
}

func (ctrl *controller) renderTaskCard(c *ui.Context, item TaskItem, nowMs int64, globalDisabled, multiHost bool, panelTaskID, panelTab string, t Translate) {
	p := theme.Of(c)
	cardDisabled := globalDisabled || item.Unavailable
	openTab := ""
	if panelTaskID == item.Task.ID {
		openTab = panelTab
	}
	borderColor := p.Border
	if openTab != "" {
		borderColor = p.Ring
	}

	ctrl.mu.Lock()
	projName := ctrl.projectNameLocked(item.Task.Project, t)
	hostLabel := ctrl.nodeLabelLocked(ctrl.executionNodeOf(item.Task))
	ctrl.mu.Unlock()

	timeLine := taskCardTimeText(item, nowMs, ctrl.loc, t)
	schedSummary := FormatScheduleSummary(item.Task.Timing, nowMs, ctrl.loc, t)

	card := kit.Frost(
		c,
		ui.Row(c.Key("task-card-"+item.Task.ID)).FillWidth().MinWidth(0).Gap(12).Padding(12, 16).
			Radius(p.RadiusLg).Border(1, borderColor).AlignItems(ui.Center),
		p.Surface,
		kit.CardTint,
	)
	card.Children(func() {
		// Left summary: Name + schedule summary + project/host metadata.
		ui.Column(c.Key("summary")).Grow(1).Shrink(1).MinWidth(0).Gap(4).Children(func() {
			ui.Text(c, item.Task.Name).FontSize(14).FontWeight(600).SingleLine().TextColor(p.SurfaceForeground)
			ui.Row(c.Key("meta")).AlignItems(ui.Center).Gap(10).MinWidth(0).Children(func() {
				ui.Row(c.Key("project")).AlignItems(ui.Center).Gap(4).Shrink(1).MinWidth(0).Children(func() {
					ui.Icon(c, icons.Folder).FontSize(12).TextColor(p.MutedForeground).Shrink(0)
					ui.Text(c, projName).FontSize(12).SingleLine().Shrink(1).MinWidth(0).TextColor(p.MutedForeground)
				})
				if multiHost {
					ui.Text(c, "· "+hostLabel).FontSize(12).SingleLine().Shrink(1).MinWidth(0).TextColor(p.MutedForeground)
				}
				ui.Text(c, "· "+schedSummary).FontSize(12).Font("monospace").SingleLine().Shrink(1).MinWidth(0).TextColor(p.MutedForeground)
			})
		})

		// Status & next/running time.
		ui.Row(c.Key("status")).Shrink(0).AlignItems(ui.Center).Gap(8).Children(func() {
			if timeLine != "" {
				ui.Text(c, timeLine).FontSize(12).SingleLine().TextColor(p.MutedForeground)
			}
			renderTaskStatusBadge(c, item, t)
		})

		// Enabled switch.
		enabled := item.Task.Enabled
		if ui.Switch(c.Key("enabled"), &enabled).Label(t("scheduled_tasks_enabled") + ": " + item.Task.Name).Disabled(cardDisabled).Changed() {
			ctrl.toggleEnabled(item, enabled)
		}

		// 5 actions: Run now, Details, Records, Edit, Delete.
		ui.Row(c.Key("actions")).Shrink(0).Gap(4).AlignItems(ui.Center).Children(func() {
			kit.Action(c.Key("run"), icons.PlayNav, t("scheduled_tasks_run"), cardDisabled).OnClick(func() {
				ctrl.runNow(item)
			})
			kit.IconButton(c.Key("details"), icons.Info, t("scheduled_tasks_details"), 28, openTab == "details").OnClick(func() {
				ctrl.inspect(item, "details")
			})
			kit.IconButton(c.Key("records"), icons.ClockNav, t("scheduled_tasks_records"), 28, openTab == "records").OnClick(func() {
				ctrl.inspect(item, "records")
			})
			kit.Action(c.Key("edit"), icons.EditNav, t("scheduled_tasks_edit"), cardDisabled).OnClick(func() {
				itemCopy := item
				ctrl.openEditor(&itemCopy)
			})
			kit.Action(c.Key("delete"), icons.TrashNav, t("scheduled_tasks_remove"), cardDisabled).OnClick(func() {
				ctrl.requestDelete(item)
			})
		})
	})
}

func taskCardTimeText(item TaskItem, nowMs int64, loc *time.Location, t Translate) string {
	if mins, ok := ElapsedMinutes(item, nowMs); ok {
		return t("scheduled_tasks_running_for", "minutes", mins)
	}
	if item.Task.Enabled && item.NextMs != nil {
		return FormatWhen(*item.NextMs, nowMs, loc, t)
	}
	return ""
}

func renderTaskStatusBadge(c *ui.Context, item TaskItem, t Translate) {
	p := theme.Of(c)
	badge := TaskStatus(item)
	label := t(badge.LabelKey)
	if badge.Outline {
		ui.Text(c, label).FontSize(12).SingleLine().Padding(1, 6).Radius(999).
			Border(1, p.Border).TextColor(p.MutedForeground)
		return
	}
	kit.Badge(c, label, badge.Tone)
}

func (ctrl *controller) renderEditorDialog(c *ui.Context, t Translate) {
	ctrl.mu.Lock()
	ed := ctrl.editor
	busy := ctrl.busy
	hasPending := ctrl.pending != nil
	nodes := slices.Clone(ctrl.nodes)
	projects := slices.Clone(ctrl.projects)
	worktrees := slices.Clone(ctrl.worktrees)
	var teams []protocol.TeamProfile
	var modelOpts []kit.ModelOption
	var fixedLead *protocol.RoleModel
	if ed != nil {
		teams = slices.Clone(ctrl.teamsBy[ed.executionNode])
		modelOpts = ctrl.modelOptionsLocked(ed.executionNode)
		fixedLead = ctrl.fixedLeadModelLocked(ed)
	}
	ctrl.mu.Unlock()

	open := ed != nil
	if !open {
		return
	}
	p := theme.Of(c)
	disabled := busy || hasPending
	title := t("scheduled_tasks_create")
	if ed.editing {
		title = t("scheduled_tasks_edit")
	}
	winW, winH := c.Size()
	maxBodyH := max(winH*0.65, 280)

	kit.Dialog(c.Key("task-editor-dialog"), &open, func() {
		kit.DialogHeader(c, title)

		ui.Scroll(c.Key("editor-scroll")).FillWidth().MaxHeight(maxBodyH).Children(func() {
			ui.Column(c).FillWidth().Gap(12).PaddingEnd(4).Children(func() {
				// 1. Name
				kit.Field(c, "field-name", t("scheduled_tasks_name"), "", func() {
					ui.TextInput(c.Key("input"), &ed.name).Label(t("scheduled_tasks_name")).
						Placeholder(t("scheduled_tasks_name_placeholder")).FillWidth().Disabled(disabled)
				})

				// 2. Prompt
				kit.Field(c, "field-prompt", t("scheduled_tasks_prompt"), "", func() {
					ui.TextArea(c.Key("input"), &ed.prompt).Label(t("scheduled_tasks_prompt")).
						Placeholder(t("scheduled_tasks_prompt_placeholder")).FillWidth().MinHeight(84).MaxHeight(160).Disabled(disabled)
				})

				// 3. Execution host (locked on edit)
				kit.Field(c, "field-host", t("scheduled_tasks_execution_host"), "", func() {
					hostOpts := make([]kit.SelectOption, 0, len(nodes)+1)
					selectedHost := ed.executionNode.String()
					foundHost := false
					for _, n := range nodes {
						id := n.ID.String()
						label := n.Label
						if label == "" {
							ctrl.mu.Lock()
							label = ctrl.nodeLabelLocked(n.ID)
							ctrl.mu.Unlock()
						}
						if !n.Connected {
							label += " (" + t("scheduled_tasks_execution_unavailable") + ")"
						}
						if id == selectedHost {
							foundHost = true
						}
						hostOpts = append(hostOpts, kit.SelectOption{Value: id, Label: label, Disabled: !n.Connected})
					}
					if !foundHost {
						label := t("scheduled_tasks_execution_unavailable")
						if ed.executionNode != (protocol.NodeID{}) {
							ctrl.mu.Lock()
							hostName := ctrl.nodeLabelLocked(ed.executionNode)
							ctrl.mu.Unlock()
							if hostName != "" {
								label = hostName + " (" + label + ")"
							}
						}
						hostOpts = append(hostOpts, kit.SelectOption{Value: selectedHost, Label: label, Disabled: true})
					}
					el, changed := kit.SelectField(c.Key("select"), "task-editor-host", &selectedHost, kit.SelectProps{
						Label:    t("scheduled_tasks_execution_host"),
						Options:  hostOpts,
						Disabled: disabled || ed.editing,
					})
					el.FillWidth()
					if changed && !ed.editing {
						for _, n := range nodes {
							if n.ID.String() == selectedHost && n.Connected {
								ctrl.mu.Lock()
								ed.executionNode = n.ID
								ed.project = nil
								ed.worktree = nil
								ed.config = nil
								ed.team = nil
								ed.models = map[protocol.RoleID]protocol.RoleModel{}
								ed.modelID = ""
								ed.effort = ""
								ctrl.mu.Unlock()
								break
							}
						}
					}
				})

				// 4. Schedule (Once / Repeat / Weekly)
				kit.Field(c, "field-schedule", t("scheduled_tasks_schedule"), "", func() {
					kinds := []string{"once", "every", "weekly"}
					idx := max(slices.Index(kinds, ed.timingKind), 0)
					labels := []string{t("scheduled_tasks_once"), t("scheduled_tasks_every"), t("scheduled_tasks_weekly")}
					if kit.Segmented(c, "timing-kind", &idx, labels, kit.SegmentedProps{Block: true, Disabled: disabled}).
						Label(t("scheduled_tasks_schedule")).Changed() {
						ed.timingKind = kinds[idx]
					}
				})

				if ed.timingKind == "weekly" {
					kit.Field(c, "field-weekdays", t("scheduled_tasks_weekdays"), "", func() {
						ui.Row(c).FillWidth().Wrap().Gap(6).Children(func() {
							for day := uint8(1); day <= 7; day++ {
								day := day
								active := slices.Contains(ed.weekdays, day)
								label := t(fmt.Sprintf("scheduled_tasks_weekday_%d", day))
								b := ui.ButtonBase(c.Key(day)).Label(label).Disabled(disabled).
									Height(30).PaddingX(10).Radius(p.Radius).Center().Cursor(ui.CursorPointer)
								if active {
									b.Background(p.ButtonPrimary).TextColor(p.OnPrimary)
								} else {
									b.Border(1, p.Border).Background(p.Surface).TextColor(p.Foreground)
								}
								b.Children(func() {
									ui.Text(c, label).FontSize(13).SingleLine()
								}).OnClick(func() {
									if active {
										ed.weekdays = slices.DeleteFunc(ed.weekdays, func(v uint8) bool { return v == day })
									} else {
										ed.weekdays = append(ed.weekdays, day)
										slices.Sort(ed.weekdays)
									}
								})
							}
						})
					})
					ui.Row(c.Key("weekly-fields")).FillWidth().Gap(10).Children(func() {
						ui.Column(c).Width(140).Children(func() {
							kit.Field(c, "field-walltime", t("scheduled_tasks_local_time"), "", func() {
								ui.TextInput(c.Key("input"), &ed.wallTime).Label(t("scheduled_tasks_local_time")).
									Placeholder(t("scheduled_tasks_time_placeholder")).FillWidth().Disabled(disabled)
							})
						})
						ui.Column(c).Grow(1).MinWidth(0).Children(func() {
							kit.Field(c, "field-timezone", t("scheduled_tasks_timezone"), "", func() {
								ui.TextInput(c.Key("input"), &ed.timezone).Label(t("scheduled_tasks_timezone")).
									Placeholder(t("scheduled_tasks_timezone_placeholder")).FillWidth().Disabled(disabled)
							})
						})
					})
				} else {
					kit.Field(c, "field-time", t("scheduled_tasks_time"), "", func() {
						ui.Row(c).FillWidth().Gap(8).Children(func() {
							ui.DateInput(c.Key("date"), &ed.startDate).Grow(1).MinWidth(0).Disabled(disabled)
							ui.TimeInput(c.Key("time"), &ed.startTime).Label(t("scheduled_tasks_time")).Width(136).Disabled(disabled)
						})
					})
					if ed.timingKind == "every" {
						kit.Field(c, "field-interval", t("scheduled_tasks_interval"), "", func() {
							ui.TextInput(c.Key("input"), &ed.intervalSec).Label(t("scheduled_tasks_interval")).
								Placeholder(t("scheduled_tasks_interval_placeholder")).FillWidth().Disabled(disabled)
						})
					}
				}

				// 5. Project
				kit.Field(c, "field-project", t("scheduled_tasks_project"), "", func() {
					projOpts := []kit.SelectOption{{Value: "none", Label: t("scheduled_tasks_no_project")}}
					selectedProj := "none"
					if ed.project != nil {
						selectedProj = ed.project.String()
					}
					foundProj := selectedProj == "none"
					for _, pr := range projects {
						if pr.Node != ed.executionNode {
							continue
						}
						id := pr.Project.ID.String()
						if id == selectedProj {
							foundProj = true
						}
						projOpts = append(projOpts, kit.SelectOption{Value: id, Label: pr.Project.Name})
					}
					if !foundProj {
						projOpts = append(projOpts, kit.SelectOption{Value: selectedProj, Label: t("scheduled_tasks_unavailable")})
					}
					el, changed := kit.SelectField(c.Key("select"), "task-editor-project", &selectedProj, kit.SelectProps{
						Label:       t("scheduled_tasks_project"),
						Placeholder: t("scheduled_tasks_project_placeholder"),
						Options:     projOpts,
						Disabled:    disabled,
					})
					el.FillWidth()
					if changed {
						if selectedProj == "none" {
							ed.project = nil
							ed.worktree = nil
						} else if parsed, err := protocol.ParseProjectID(selectedProj); err == nil {
							ed.project = &parsed
							ed.worktree = nil
						}
					}
				})

				// 6. Worktree (when project selected)
				if ed.project != nil {
					kit.Field(c, "field-worktree", t("scheduled_tasks_worktree"), "", func() {
						wtOpts := []kit.SelectOption{{Value: "main", Label: t("scheduled_tasks_main_worktree")}}
						selectedWT := "main"
						if ed.worktree != nil {
							selectedWT = ed.worktree.String()
						}
						foundWT := selectedWT == "main"
						for _, wt := range worktrees {
							if wt.Node != ed.executionNode || wt.Worktree.Project == nil || *wt.Worktree.Project != *ed.project {
								continue
							}
							id := wt.Worktree.ID.String()
							if id == selectedWT {
								foundWT = true
							}
							wtOpts = append(wtOpts, kit.SelectOption{Value: id, Label: wt.Worktree.Path})
						}
						if !foundWT {
							wtOpts = append(wtOpts, kit.SelectOption{Value: selectedWT, Label: t("scheduled_tasks_unavailable")})
						}
						el, changed := kit.SelectField(c.Key("select"), "task-editor-worktree", &selectedWT, kit.SelectProps{
							Label:       t("scheduled_tasks_worktree"),
							Placeholder: t("scheduled_tasks_worktree_placeholder"),
							Options:     wtOpts,
							Disabled:    disabled,
						})
						el.FillWidth()
						if changed {
							if selectedWT == "main" {
								ed.worktree = nil
							} else if parsed, err := protocol.ParseWorktreeID(selectedWT); err == nil {
								ed.worktree = &parsed
							}
						}
					})
				}

				// 7. Execution (Model / Team)
				kit.Field(c, "field-execution", t("scheduled_tasks_execution"), "", func() {
					execModes := []protocol.ConversationExecution{protocol.ConversationExecutionModel, protocol.ConversationExecutionTeam}
					idx := max(slices.Index(execModes, ed.execution), 0)
					labels := []string{t("scheduled_tasks_model"), t("scheduled_tasks_team")}
					if kit.Segmented(c, "execution-mode", &idx, labels, kit.SegmentedProps{Block: true, Disabled: disabled}).
						Label(t("scheduled_tasks_execution")).Changed() {
						ctrl.mu.Lock()
						ed.execution = execModes[idx]
						ed.team = nil
						ed.models = map[protocol.RoleID]protocol.RoleModel{}
						ctrl.syncEditorLeadModelLocked(ed)
						ctrl.mu.Unlock()
					}
				})

				// 8. Team (when Execution == Team)
				if ed.execution == protocol.ConversationExecutionTeam {
					kit.Field(c, "field-team", t("scheduled_tasks_team"), "", func() {
						teamOpts := []kit.SelectOption{{Value: "default", Label: t("scheduled_tasks_default_team")}}
						selectedTeam := "default"
						if ed.team != nil {
							selectedTeam = ed.team.ID.String()
						}
						foundTeam := selectedTeam == "default"
						for _, tm := range teams {
							id := tm.ID.String()
							if id == selectedTeam {
								foundTeam = true
							}
							teamOpts = append(teamOpts, kit.SelectOption{Value: id, Label: tm.Name})
						}
						if !foundTeam {
							teamOpts = append(teamOpts, kit.SelectOption{Value: selectedTeam, Label: t("scheduled_tasks_unavailable")})
						}
						el, changed := kit.SelectField(c.Key("select"), "task-editor-team", &selectedTeam, kit.SelectProps{
							Label:       t("scheduled_tasks_team"),
							Placeholder: t("scheduled_tasks_team_placeholder"),
							Options:     teamOpts,
							Disabled:    disabled,
						})
						el.FillWidth()
						if changed {
							ctrl.mu.Lock()
							if selectedTeam == "default" {
								ed.team = nil
							} else if parsed, err := protocol.ParseTeamID(selectedTeam); err == nil {
								ed.team = &protocol.TeamSelection{ID: parsed, Revision: nil}
							}
							ed.models = map[protocol.RoleID]protocol.RoleModel{}
							ctrl.syncEditorLeadModelLocked(ed)
							ctrl.mu.Unlock()
						}
					})
				}

				// 9. Model (kit.ModelSelect)
				kit.Field(c, "field-model", t("scheduled_tasks_model"), "", func() {
					chosenModel := ed.modelID
					chosenEffort := ed.effort
					storedName := ""
					if fixedLead != nil {
						chosenModel = fixedLead.Provider.String() + "/" + fixedLead.Model
						storedName = fixedLead.Model
						if fixedLead.Effort != nil {
							chosenEffort = effortString(*fixedLead.Effort)
						}
					} else if ed.config != nil {
						storedName = ed.config.Model
					}
					kit.ModelSelect(c.Key("model-select"), kit.ModelSelectProps{
						Label:       t("scheduled_tasks_model"),
						Placeholder: t("scheduled_tasks_model_placeholder"),
						Models:      modelOpts,
						Model:       chosenModel,
						Effort:      chosenEffort,
						Stored:      storedName,
						Disabled:    disabled || fixedLead != nil,
						OnChange: func(model, effort string) {
							ctrl.mu.Lock()
							ok := ctrl.selectEditorModelLocked(ed, model, effort)
							if !ok {
								ctrl.pushToastLocked(kit.Failed, "scheduled_tasks_model_failed")
							}
							ctrl.mu.Unlock()
						},
					}).FillWidth()
				})
			})
		})

		// Footer buttons.
		kit.DialogFooter(c, func() {
			kit.Secondary(c.Key("cancel"), t("scheduled_tasks_cancel")).Disabled(busy).OnClick(ctrl.closeEditor)
			saveLabel := t("scheduled_tasks_save")
			if hasPending {
				saveLabel = t("scheduled_tasks_retry")
			}
			kit.PrimaryBusy(c.Key("save"), saveLabel, false, busy).OnClick(ctrl.saveEditor)
		})
	}).Width(min(editorDialogWidth, max(winW-48, 280))).Label(title)

	if !open && !busy {
		ctrl.closeEditor()
	}
}

func (ctrl *controller) renderDeleteConfirm(c *ui.Context, t Translate) {
	ctrl.mu.Lock()
	deleting := ctrl.deleting
	ctrl.mu.Unlock()
	if deleting == nil {
		return
	}
	open := true
	answer := kit.Confirm(c.Key("delete-confirm-"+deleting.Task.ID), &open, kit.Confirmation{
		Title:       t("scheduled_tasks_delete_title"),
		Message:     t("scheduled_tasks_delete_message", "name", deleting.Task.Name),
		Buttons:     []string{t("scheduled_tasks_cancel"), t("scheduled_tasks_remove")},
		Default:     0,
		Cancel:      0,
		Destructive: []int{1},
	})
	if answer == 1 {
		ctrl.confirmDelete()
	} else if answer == 0 || !open {
		ctrl.mu.Lock()
		ctrl.deleting = nil
		ctrl.mu.Unlock()
	}
}
