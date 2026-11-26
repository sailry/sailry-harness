package remindersdesktop

import (
	"fmt"
	"slices"
	"strconv"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	detailDefaultWidth = 380
	detailMinWidth     = 320
)

func (ctrl *controller) currentPanelItemLocked() (ReminderItem, string, []protocol.DispatchJob, bool, bool) {
	if ctrl.panel == nil {
		return ReminderItem{}, "", nil, false, false
	}
	for _, it := range ctrl.items {
		if it.Reminder.ID == ctrl.panel.reminderID {
			loaded := ctrl.historyID == it.Reminder.ID && ctrl.historyLoaded
			var jobs []protocol.DispatchJob
			if loaded {
				for _, job := range ctrl.history {
					if job.Handler == it.Reminder.ID {
						jobs = append(jobs, job)
					}
				}
			}
			return it, ctrl.panel.tab, jobs, loaded, true
		}
	}
	return ReminderItem{}, "", nil, false, false
}

func (ctrl *controller) renderDetailsHeader(c *ui.Context) {
	t := func(key string, args ...any) string { return i18n.T(c, key, args...) }
	ctrl.mu.Lock()
	_, tab, jobs, loaded, ok := ctrl.currentPanelItemLocked()
	ctrl.mu.Unlock()
	if !ok {
		return
	}

	recordsLabel := t("reminders_records")
	if loaded {
		recordsLabel = t("reminders_records_count", "count", len(jobs))
	}
	tabs := []string{"details", "records"}
	idx := max(slices.Index(tabs, tab), 0)

	ui.Row(c.Key("reminder-panel-header")).FillWidth().MinWidth(0).AlignItems(ui.Center).Justify(ui.SpaceBetween).Gap(8).Children(func() {
		if kit.Segmented(c, "tabs", &idx, []string{t("reminders_details"), recordsLabel}, kit.SegmentedProps{Size: kit.SizeSmall}).
			Label(t("reminders_details")).Changed() {
			ctrl.setPanelTab(tabs[idx])
		}
		kit.IconButton(c.Key("close"), icons.Close, t("reminders_close"), 28, false).OnClick(ctrl.closePanel)
	})
}

func (ctrl *controller) renderDetailsBody(c *ui.Context) {
	t := func(key string, args ...any) string { return i18n.T(c, key, args...) }
	p := theme.Of(c)

	ctrl.mu.Lock()
	item, tab, jobs, loaded, ok := ctrl.currentPanelItemLocked()
	projName := ctrl.projectNameLocked(item.Reminder.Project, t)
	ctrl.mu.Unlock()
	if !ok {
		return
	}

	nowMs := ctrl.now().UnixMilli()
	status := ReminderStatus(item.Reminder, nowMs)

	ui.Scroll(c.Key("reminder-panel-scroll-" + item.Reminder.ID)).Fill().Children(func() {
		ui.Column(c).FillWidth().MinWidth(0).Children(func() {
			// Summary header.
			ui.Column(c.Key("summary")).FillWidth().Gap(8).Padding(16, 16, 12, 16).
				BorderWidth(0, 0, 1, 0).BorderColor(p.Border).Children(func() {
				ui.Row(c).FillWidth().MinWidth(0).AlignItems(ui.Center).Gap(8).Children(func() {
					ui.Text(c, item.Reminder.Title).FontSize(16).FontWeight(600).SingleLine().Grow(1).Shrink(1).MinWidth(0).TextColor(p.Foreground)
					kit.Badge(c, t(status.LabelKey), status.Tone)
				})
				ui.Row(c).AlignItems(ui.Center).Gap(4).MinWidth(0).Children(func() {
					ui.Icon(c, icons.Folder).FontSize(12).TextColor(p.MutedForeground).Shrink(0)
					ui.Text(c, projName).FontSize(12).SingleLine().Shrink(1).MinWidth(0).TextColor(p.MutedForeground)
				})
			})

			if tab == "records" {
				ctrl.renderRecordsTab(c, jobs, loaded, nowMs, t)
			} else {
				ctrl.renderDetailsTab(c, item, nowMs, projName, t)
			}
		})
	})
}

func (ctrl *controller) renderDetailsTab(c *ui.Context, item ReminderItem, nowMs int64, projName string, t Translate) {
	p := theme.Of(c)
	late := IsOverdue(item.Reminder, nowMs)

	ui.Column(c.Key("overview")).FillWidth().Gap(16).Padding(12, 16, 24, 16).Children(func() {
		// Colored calendar tile card.
		bandColor := p.Primary
		bandText := p.PrimaryForeground
		switch {
		case item.Reminder.DueMs == nil:
			bandColor = p.MutedForeground
			bandText = p.Background
		case item.Reminder.Completed:
			bandColor = p.Success
		case late:
			bandColor = p.Danger
		}
		monthStr := "—"
		dayStr := "—"
		if item.Reminder.DueMs != nil {
			dt := time.UnixMilli(*item.Reminder.DueMs).In(ctrl.loc)
			monthStr = t(fmt.Sprintf("reminders_month_%d", int(dt.Month())))
			dayStr = strconv.Itoa(dt.Day())
		}

		ui.Row(c.Key("due-card")).FillWidth().AlignItems(ui.Center).Gap(12).Padding(12).Radius(p.Radius).
			Border(1, p.Border).Background(p.Muted).Children(func() {
			ui.Column(c.Key("tile")).Width(46).Shrink(0).Radius(p.Radius).Border(1, p.Border).Background(p.Background).Clip().AlignItems(ui.Center).Children(func() {
				ui.Row(c).FillWidth().Center().Padding(2, 0).Background(bandColor).Children(func() {
					ui.Text(c, monthStr).FontSize(11).FontWeight(600).TextColor(bandText)
				})
				ui.Text(c, dayStr).FontSize(16).FontWeight(700).Font("monospace").Padding(4, 0).TextColor(p.Foreground)
			})
			ui.Column(c).Grow(1).Shrink(1).MinWidth(0).Gap(4).Children(func() {
				if item.Reminder.DueMs == nil {
					ui.Text(c, t("reminders_no_due")).FontSize(14).FontWeight(500).TextColor(p.Foreground)
					ui.Text(c, t("reminders_no_notice")).FontSize(12).TextColor(p.MutedForeground)
				} else {
					at := *item.Reminder.DueMs
					ui.Text(c, FormatDate(at, ctrl.loc, t)+" "+FormatClock(at, false, ctrl.loc)).
						FontSize(14).FontWeight(500).Font("monospace").TextColor(p.Foreground)
					relCol := p.MutedForeground
					if late {
						relCol = p.Danger
					}
					ui.Text(c, FormatRelative(at, nowMs, t)).FontSize(12).TextColor(relCol)
				}
			})
		})

		// Facts: Status, Reminder notice, Project.
		statusLabel := t("reminders_pending")
		statusDot := p.Warning
		if item.Reminder.Completed {
			statusLabel = t("reminders_completed")
			statusDot = p.Success
		}
		noticeLabel := t("reminders_none")
		noticeNote := ""
		var noticeDot *ui.Color
		if item.Reminder.NotifiedMs != nil {
			noticeLabel = t("reminders_notified")
			noticeNote = FormatWhen(*item.Reminder.NotifiedMs, nowMs, ctrl.loc, t)
			col := p.Success
			noticeDot = &col
		} else if item.Reminder.DueMs != nil && !item.Reminder.Completed {
			noticeLabel = t("reminders_waiting")
			col := p.Info
			noticeDot = &col
		}

		ui.Column(c.Key("facts")).FillWidth().Gap(2).Padding(12, 0, 0, 0).BorderWidth(1, 0, 0, 0).BorderColor(p.Border).Children(func() {
			renderFactRow(c, "status", t("reminders_status"), statusLabel, "", &statusDot, nil)
			renderFactRow(c, "notice", t("reminders_reminder"), noticeLabel, noticeNote, noticeDot, nil)
			renderFactRow(c, "project", t("reminders_project"), projName, "", nil, icons.Folder)
		})

		// Notes.
		ui.Box(c.Key("notes-section")).FillWidth().Padding(12, 0, 0, 0).BorderWidth(1, 0, 0, 0).BorderColor(p.Border).Children(func() {
			if item.Reminder.Message != "" {
				ui.Box(c).FillWidth().Radius(p.Radius).Background(p.Muted).Padding(10, 12).Children(func() {
					ui.Text(c, item.Reminder.Message).FontSize(13).TextColor(p.Foreground)
				})
			} else {
				ui.Text(c, t("reminders_no_notes")).FontSize(13).TextColor(p.MutedForeground)
			}
		})
	})
}

func renderFactRow(c *ui.Context, key, label, value, note string, dot *ui.Color, icon *ui.SVG) {
	p := theme.Of(c)
	ui.Row(c.Key(key)).FillWidth().MinHeight(28).AlignItems(ui.Center).Gap(12).Children(func() {
		ui.Text(c, label).Width(76).Shrink(0).FontSize(13).TextColor(p.MutedForeground)
		ui.Row(c).Grow(1).Shrink(1).MinWidth(0).AlignItems(ui.Center).Gap(6).Children(func() {
			if dot != nil {
				ui.Box(c).Size(8, 8).Shrink(0).Radius(4).Background(*dot)
			}
			if icon != nil {
				ui.Icon(c, icon).FontSize(13).TextColor(p.MutedForeground).Shrink(0)
			}
			ui.Text(c, value).FontSize(13).SingleLine().Shrink(1).MinWidth(0).TextColor(p.Foreground)
			if note != "" {
				ui.Text(c, note).FontSize(12).SingleLine().Shrink(0).TextColor(p.MutedForeground)
			}
		})
	})
}

func (ctrl *controller) renderRecordsTab(c *ui.Context, jobs []protocol.DispatchJob, loaded bool, nowMs int64, t Translate) {
	p := theme.Of(c)
	if !loaded {
		ui.Row(c.Key("records-loading")).FillWidth().Height(96).Center().Children(func() {
			kit.Loader(c, 22)
		})
		return
	}
	if len(jobs) == 0 {
		ui.Box(c.Key("records-empty")).FillWidth().Padding(12).Children(func() {
			kit.Result(c, kit.ResultProps{Icon: icons.Bell, Title: t("reminders_no_records")})
		})
		return
	}

	ui.Column(c.Key("records-timeline")).FillWidth().Padding(12, 12, 24, 12).Children(func() {
		lastDay := ""
		for i, job := range jobs {
			at := DeliveryTime(job)
			day := FormatDayLabel(at, nowMs, ctrl.loc, t)
			if day != lastDay {
				lastDay = day
				renderTimelineRail(c, fmt.Sprintf("day-%d", i), false, 10, func() {
					ui.Box(c).Margin(4, 0, 0, 0).Size(8, 8).Shrink(0).Radius(4).Border(2, p.Border).Background(p.Background)
				}, func() {
					ui.Text(c, day).FontSize(12).FontWeight(600).TextColor(p.MutedForeground)
				})
			}

			isLast := i == len(jobs)-1
			failed := job.Status == protocol.DispatchStatusFailed
			renderTimelineRail(c, "job-"+job.ID.String(), isLast, 12, func() {
				ui.Box(c).Margin(11, 0, 0, 0).Size(12, 12).Shrink(0).Radius(6).Background(deliveryStatusColor(job.Status, p))
			}, func() {
				borderCol := p.Border
				bgCol := p.Muted
				titleCol := p.Foreground
				if failed {
					borderCol = p.Danger.Alpha(0.25)
					bgCol = p.Danger.Alpha(0.08)
					titleCol = p.Danger
				}
				ui.Column(c).FillWidth().MinWidth(0).Gap(6).Padding(8, 10).Radius(p.Radius).Border(1, borderCol).Background(bgCol).Children(func() {
					ui.Row(c).FillWidth().AlignItems(ui.Center).Gap(8).Children(func() {
						ui.Text(c, t(DeliveryLabelKey(job.Status))).FontSize(13).FontWeight(500).TextColor(titleCol)
						ui.Text(c, FormatClock(at, true, ctrl.loc)).FontSize(12).Font("monospace").TextColor(p.MutedForeground)
					})
					if job.Error != nil && job.Error.Message != "" {
						errCol := p.MutedForeground
						if failed {
							errCol = p.Danger
						}
						ui.Text(c, job.Error.Message).FontSize(12).TextColor(errCol)
					}
				})
			})
		}
	})
}

func renderTimelineRail(c *ui.Context, key string, last bool, bottomPad float32, nodeFn, bodyFn func()) {
	p := theme.Of(c)
	ui.Row(c.Key(key)).FillWidth().AlignItems(ui.Stretch).Gap(10).Children(func() {
		ui.Column(c).Width(14).Shrink(0).AlignItems(ui.Center).Children(func() {
			nodeFn()
			if !last {
				ui.Box(c).Grow(1).Width(2).MinHeight(10).Margin(4, 0, 0, 0).Radius(1).Background(p.Border)
			}
		})
		ui.Box(c).Grow(1).Shrink(1).MinWidth(0).Padding(0, 0, bottomPad, 0).Children(bodyFn)
	})
}

func deliveryStatusColor(status protocol.DispatchStatus, p *theme.Palette) ui.Color {
	switch status {
	case protocol.DispatchStatusCompleted:
		return p.Success
	case protocol.DispatchStatusFailed:
		return p.Danger
	case protocol.DispatchStatusRunning, protocol.DispatchStatusQueued:
		return p.Info
	case protocol.DispatchStatusUnknown:
		return p.Warning
	default:
		return p.MutedForeground
	}
}
