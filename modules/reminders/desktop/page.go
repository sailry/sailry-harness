package remindersdesktop

import (
	"slices"
	"strconv"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const editorDialogWidth = 480

func (ctrl *controller) render(c *ui.Context) {
	ctrl.flushToasts(c)
	t := func(key string, args ...any) string { return i18n.T(c, key, args...) }
	p := theme.Of(c)

	ctrl.mu.Lock()
	loaded := ctrl.loaded
	loading := ctrl.loading
	busy := ctrl.busy
	pending := ctrl.pending
	projFilter := ctrl.projFilter
	showCompleted := ctrl.showCompleted
	items := slices.Clone(ctrl.items)
	projects := slices.Clone(ctrl.projects)
	var panelID, panelTab string
	if ctrl.panel != nil {
		panelID, panelTab = ctrl.panel.reminderID, ctrl.panel.tab
	}
	hasEditor := ctrl.editor != nil
	hasDelete := ctrl.deleting != nil
	ctrl.mu.Unlock()

	nowMs := ctrl.now().UnixMilli()
	disabled := busy || pending != nil

	filtered := make([]ReminderItem, 0, len(items))
	for _, it := range items {
		switch projFilter {
		case "all":
			filtered = append(filtered, it)
		case "none":
			if it.Reminder.Project == nil {
				filtered = append(filtered, it)
			}
		default:
			if it.Reminder.Project != nil && it.Reminder.Project.String() == projFilter {
				filtered = append(filtered, it)
			}
		}
	}
	ordered := SortReminders(filtered)
	var openItems, doneItems []ReminderItem
	for _, it := range ordered {
		if it.Reminder.Completed {
			doneItems = append(doneItems, it)
		} else {
			openItems = append(openItems, it)
		}
	}

	kit.Page(c, "reminders-page", t("reminders_page_title"), t("reminders_description"), func() {
		// Toolbar: Project filter + New reminder button.
		ui.Row(c.Key("toolbar")).FillWidth().Wrap().AlignItems(ui.Center).Justify(ui.SpaceBetween).Gap(12).Children(func() {
			filterOpts := ctrl.projectFilterOptions(projects, items, t)
			selectedFilter := projFilter
			el, changed := kit.SelectField(c.Key("project-filter"), "reminders-project-filter", &selectedFilter, kit.SelectProps{
				Label:       t("reminders_project"),
				Placeholder: t("reminders_all_projects"),
				Options:     filterOpts,
			})
			el.Width(200).MaxWidthPercent(100)
			if changed {
				ctrl.mu.Lock()
				ctrl.projFilter = selectedFilter
				ctrl.mu.Unlock()
			}
			kit.Primary(c.Key("create"), t("reminders_create"), disabled).Children(func() {
				ui.Text(c, t("reminders_create")).FontSize(14).SingleLine()
			}).OnClick(func() {
				ctrl.openEditor(nil)
			})
		})

		// Uncertain durable mutation recovery banner.
		if !hasEditor && !hasDelete && pending != nil {
			msgKey := pending.errorKey
			if msgKey == "" {
				msgKey = "reminders_unknown"
			}
			kit.Alert(c, "pending-banner", kit.Caution, t(msgKey), func() {
				ui.Row(c).Shrink(0).Gap(8).Children(func() {
					kit.SecondaryBusy(c.Key("retry"), t("reminders_retry"), busy).OnClick(ctrl.retryPending)
					kit.Secondary(c.Key("cancel"), t("reminders_cancel")).Disabled(busy).OnClick(ctrl.dismissPending)
				})
			})
		}

		if !loaded && loading {
			kit.Card(c, func() {
				ui.Row(c).FillWidth().Height(96).Center().Children(func() {
					kit.Loader(c, 24)
				})
			})
			return
		}

		// Open list (hidden when only completed reminders exist).
		if len(openItems) > 0 || len(doneItems) == 0 {
			if len(openItems) == 0 {
				kit.Result(c, kit.ResultProps{
					Icon:  icons.Bell,
					Title: t("reminders_empty"),
					Card:  true,
				})
			} else {
				ui.Column(c.Key("open-list")).FillWidth().Gap(10).Children(func() {
					for _, item := range openItems {
						ctrl.renderReminderRow(c, item, nowMs, disabled, panelID, panelTab, t)
					}
				})
			}
		}

		// Collapsible Completed (N) section.
		if len(doneItems) > 0 {
			ui.Column(c.Key("completed-section")).FillWidth().Gap(8).Children(func() {
				chev := icons.ChevronRight
				if showCompleted {
					chev = icons.ChevronDown
				}
				label := t("reminders_completed") + " " + strconv.Itoa(len(doneItems))
				kit.Ghost(c.Key("completed-toggle"), label, false).Height(28).PaddingX(8).Gap(6).AlignSelf(ui.Start).Children(func() {
					ui.Icon(c, chev).FontSize(14).TextColor(p.MutedForeground)
					ui.Text(c, t("reminders_completed")).FontSize(13).FontWeight(500).TextColor(p.Foreground)
					ui.Text(c, strconv.Itoa(len(doneItems))).FontSize(13).TextColor(p.MutedForeground)
				}).OnClick(func() {
					ctrl.mu.Lock()
					ctrl.showCompleted = !ctrl.showCompleted
					ctrl.mu.Unlock()
				})
				if showCompleted {
					ui.Column(c.Key("done-list")).FillWidth().Gap(10).Children(func() {
						for _, item := range doneItems {
							ctrl.renderReminderRow(c, item, nowMs, disabled, panelID, panelTab, t)
						}
					})
				}
			})
		}
	})

	ctrl.renderEditorDialog(c, t)
	ctrl.renderDeleteConfirm(c, t)
}

func (ctrl *controller) projectFilterOptions(projects []projectOption, items []ReminderItem, t Translate) []kit.SelectOption {
	opts := []kit.SelectOption{
		{Value: "all", Label: t("reminders_all_projects")},
		{Value: "none", Label: t("reminders_no_project")},
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
		if it.Reminder.Project == nil {
			continue
		}
		id := it.Reminder.Project.String()
		if seen[id] {
			continue
		}
		seen[id] = true
		opts = append(opts, kit.SelectOption{Value: id, Label: t("reminders_unavailable_project")})
	}
	return opts
}

func (ctrl *controller) renderReminderRow(c *ui.Context, item ReminderItem, nowMs int64, globalDisabled bool, panelID, panelTab string, t Translate) {
	p := theme.Of(c)
	rowDisabled := globalDisabled || item.Unavailable
	openTab := ""
	if panelID == item.Reminder.ID {
		openTab = panelTab
	}
	borderColor := p.Border
	if openTab != "" {
		borderColor = p.Ring
	}
	late := IsOverdue(item.Reminder, nowMs)

	ctrl.mu.Lock()
	projName := ctrl.projectNameLocked(item.Reminder.Project, t)
	ctrl.mu.Unlock()

	card := kit.Frost(
		c,
		ui.Row(c.Key("reminder-row-"+item.Reminder.ID)).FillWidth().MinWidth(0).Gap(12).Padding(12, 16).
			Radius(p.RadiusLg).Border(1, borderColor).AlignItems(ui.Center),
		p.Surface,
		kit.CardTint,
	)
	card.Children(func() {
		// Completion checkbox.
		completed := item.Reminder.Completed
		if ui.Checkbox(c.Key("complete"), &completed, "").Label(t("reminders_complete") + ": " + item.Reminder.Title).Disabled(rowDisabled).Changed() {
			ctrl.toggleCompleted(item, completed)
		}

		// Title (strikethrough when completed) + project subtitle.
		ui.Column(c.Key("summary")).Grow(1).Shrink(1).MinWidth(0).Gap(4).Children(func() {
			titleEl := ui.Text(c, item.Reminder.Title).FontSize(14).FontWeight(600).SingleLine()
			if item.Reminder.Completed {
				titleEl.Strikethrough().TextColor(p.MutedForeground)
			} else {
				titleEl.TextColor(p.SurfaceForeground)
			}
			ui.Row(c.Key("project")).AlignItems(ui.Center).Gap(4).MinWidth(0).Children(func() {
				ui.Icon(c, icons.Folder).FontSize(12).TextColor(p.MutedForeground).Shrink(0)
				ui.Text(c, projName).FontSize(12).SingleLine().Shrink(1).MinWidth(0).TextColor(p.MutedForeground)
			})
		})

		// Time / Overdue tag.
		ui.Row(c.Key("time")).Shrink(0).AlignItems(ui.Center).Gap(8).Children(func() {
			timeText := t("reminders_no_time")
			if item.Unavailable {
				timeText = t("reminders_execution_unavailable")
			} else if item.Reminder.DueMs != nil {
				timeText = FormatWhen(*item.Reminder.DueMs, nowMs, ctrl.loc, t)
			}
			timeCol := p.MutedForeground
			if late {
				timeCol = p.Danger
			}
			ui.Text(c, timeText).FontSize(12).SingleLine().TextColor(timeCol)
			if late {
				kit.Badge(c, t("reminders_overdue"), kit.Negative)
			}
		})

		// 4 actions: Details, Records, Edit, Delete.
		ui.Row(c.Key("actions")).Shrink(0).Gap(4).AlignItems(ui.Center).Children(func() {
			kit.IconButton(c.Key("details"), icons.Info, t("reminders_details"), 28, openTab == "details").OnClick(func() {
				ctrl.inspect(item, "details")
			})
			kit.IconButton(c.Key("records"), icons.ClockNav, t("reminders_records"), 28, openTab == "records").OnClick(func() {
				ctrl.inspect(item, "records")
			})
			kit.Action(c.Key("edit"), icons.EditNav, t("reminders_edit"), rowDisabled).OnClick(func() {
				cp := item
				ctrl.openEditor(&cp)
			})
			kit.Action(c.Key("delete"), icons.TrashNav, t("reminders_remove"), rowDisabled).OnClick(func() {
				ctrl.requestDelete(item)
			})
		})
	})
}

func (ctrl *controller) renderEditorDialog(c *ui.Context, t Translate) {
	ctrl.mu.Lock()
	ed := ctrl.editor
	busy := ctrl.busy
	hasPending := ctrl.pending != nil
	nodes := slices.Clone(ctrl.nodes)
	projects := slices.Clone(ctrl.projects)
	ctrl.mu.Unlock()

	open := ed != nil
	if !open {
		return
	}
	disabled := busy || hasPending
	title := t("reminders_create")
	if ed.editing {
		title = t("reminders_edit")
	}
	winW, _ := c.Size()

	kit.Dialog(c.Key("reminder-editor-dialog"), &open, func() {
		kit.DialogHeader(c, title)

		ui.Column(c).FillWidth().Gap(12).Children(func() {
			// 1. Title
			kit.Field(c, "field-title", t("reminders_title"), "", func() {
				ui.TextInput(c.Key("input"), &ed.title).Label(t("reminders_title")).
					Placeholder(t("reminders_title_placeholder")).FillWidth().Disabled(disabled)
			})

			// 2. Notes
			kit.Field(c, "field-message", t("reminders_message"), "", func() {
				ui.TextArea(c.Key("input"), &ed.message).Label(t("reminders_message")).
					Placeholder(t("reminders_message_placeholder")).FillWidth().MinHeight(80).MaxHeight(160).Disabled(disabled)
			})

			// 3. Execution host (locked on edit)
			kit.Field(c, "field-host", t("reminders_execution_host"), "", func() {
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
						label += " (" + t("reminders_execution_unavailable") + ")"
					}
					if id == selectedHost {
						foundHost = true
					}
					hostOpts = append(hostOpts, kit.SelectOption{Value: id, Label: label, Disabled: !n.Connected})
				}
				if !foundHost {
					label := t("reminders_execution_unavailable")
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
				el, changed := kit.SelectField(c.Key("select"), "reminder-editor-host", &selectedHost, kit.SelectProps{
					Label:    t("reminders_execution_host"),
					Options:  hostOpts,
					Disabled: disabled || ed.editing,
				})
				el.FillWidth()
				if changed && !ed.editing {
					for _, n := range nodes {
						if n.ID.String() == selectedHost && n.Connected {
							ed.executionNode = n.ID
							ed.hasExecNode = true
							// Clear project if it does not belong to the newly chosen host.
							if ed.project != nil {
								belongs := false
								for _, pr := range projects {
									if pr.Node == n.ID && pr.Project.ID == *ed.project {
										belongs = true
										break
									}
								}
								if !belongs {
									ed.project = nil
								}
							}
							break
						}
					}
				}
			})

			// 4. Project
			kit.Field(c, "field-project", t("reminders_project"), "", func() {
				projOpts := []kit.SelectOption{{Value: "none", Label: t("reminders_no_project")}}
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
					projOpts = append(projOpts, kit.SelectOption{Value: selectedProj, Label: t("reminders_unavailable_project")})
				}
				el, changed := kit.SelectField(c.Key("select"), "reminder-editor-project", &selectedProj, kit.SelectProps{
					Label:       t("reminders_project"),
					Placeholder: t("reminders_project_placeholder"),
					Options:     projOpts,
					Disabled:    disabled,
				})
				el.FillWidth()
				if changed {
					if selectedProj == "none" {
						ed.project = nil
					} else if parsed, err := protocol.ParseProjectID(selectedProj); err == nil {
						ed.project = &parsed
					}
				}
			})

			// 5. Remind me checkbox + DateInput/TimeInput
			ui.Checkbox(c.Key("timed"), &ed.timed, t("reminders_timed")).Disabled(disabled)
			if ed.timed {
				kit.Field(c, "field-time", t("reminders_time"), "", func() {
					ui.Row(c).FillWidth().Gap(8).Children(func() {
						ui.DateInput(c.Key("date"), &ed.dueDate).Grow(1).MinWidth(0).Disabled(disabled)
						ui.TimeInput(c.Key("time"), &ed.dueTime).Label(t("reminders_time")).Width(136).Disabled(disabled)
					})
				})
			}
		})

		// Footer.
		kit.DialogFooter(c, func() {
			kit.Secondary(c.Key("cancel"), t("reminders_cancel")).Disabled(busy).OnClick(ctrl.closeEditor)
			saveLabel := t("reminders_save")
			if hasPending {
				saveLabel = t("reminders_retry")
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
	answer := kit.Confirm(c.Key("delete-confirm-"+deleting.Reminder.ID), &open, kit.Confirmation{
		Title:       t("reminders_delete_title"),
		Message:     t("reminders_delete_message", "title", deleting.Reminder.Title),
		Buttons:     []string{t("reminders_cancel"), t("reminders_remove")},
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
