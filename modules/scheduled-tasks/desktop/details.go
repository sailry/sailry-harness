package scheduledtasksdesktop

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

func (ctrl *controller) currentPanelItemLocked() (TaskItem, string, []protocol.ScheduledTaskHistoryEntry, bool, bool, bool) {
	if ctrl.panel == nil {
		return TaskItem{}, "", nil, false, false, false
	}
	for _, it := range ctrl.items {
		if it.Task.ID == ctrl.panel.taskID {
			loaded := ctrl.historyTaskID == it.Task.ID && ctrl.historyLoaded
			var runs []protocol.ScheduledTaskHistoryEntry
			if loaded {
				for _, r := range ctrl.history {
					if r.Job.Handler == it.Task.ID {
						runs = append(runs, r)
					}
				}
			}
			return it, ctrl.panel.tab, runs, loaded, ctrl.historyMore, true
		}
	}
	return TaskItem{}, "", nil, false, false, false
}

func (ctrl *controller) renderDetailsHeader(c *ui.Context) {
	t := func(key string, args ...any) string { return i18n.T(c, key, args...) }
	ctrl.mu.Lock()
	_, tab, runs, loaded, more, ok := ctrl.currentPanelItemLocked()
	ctrl.mu.Unlock()
	if !ok {
		return
	}

	recordsLabel := t("scheduled_tasks_records")
	if loaded {
		if more {
			recordsLabel = t("scheduled_tasks_records_more_count", "count", len(runs))
		} else {
			recordsLabel = t("scheduled_tasks_records_count", "count", len(runs))
		}
	}
	tabs := []string{"details", "records"}
	idx := max(slices.Index(tabs, tab), 0)

	ui.Row(c.Key("task-panel-header")).FillWidth().MinWidth(0).AlignItems(ui.Center).Justify(ui.SpaceBetween).Gap(8).Children(func() {
		if kit.Segmented(c, "tabs", &idx, []string{t("scheduled_tasks_details"), recordsLabel}, kit.SegmentedProps{Size: kit.SizeSmall}).
			Label(t("scheduled_tasks_details")).Changed() {
			ctrl.setPanelTab(tabs[idx])
		}
		kit.IconButton(c.Key("close"), icons.Close, t("scheduled_tasks_close"), 28, false).OnClick(ctrl.closePanel)
	})
}

func (ctrl *controller) renderDetailsBody(c *ui.Context) {
	t := func(key string, args ...any) string { return i18n.T(c, key, args...) }
	p := theme.Of(c)

	ctrl.mu.Lock()
	item, tab, runs, loaded, more, ok := ctrl.currentPanelItemLocked()
	busy := ctrl.busy
	hasPending := ctrl.pending != nil
	loading := ctrl.loading
	projName := ctrl.projectNameLocked(item.Task.Project, t)
	wtPath := ctrl.worktreePathLocked(item.Task.Worktree, t)
	execNode := ctrl.executionNodeOf(item.Task)
	teamName := ctrl.teamNameLocked(execNode, item.Task.Team, t)
	hostLabel := ctrl.nodeLabelLocked(execNode)
	multiHost := len(ctrl.nodes) > 1
	ctrl.mu.Unlock()
	if !ok {
		return
	}

	nowMs := ctrl.now().UnixMilli()

	ui.Scroll(c.Key("task-panel-scroll-" + item.Task.ID)).Fill().Children(func() {
		ui.Column(c).FillWidth().MinWidth(0).Children(func() {
			// Panel summary header.
			ui.Column(c.Key("summary")).FillWidth().Gap(8).Padding(16, 16, 12, 16).
				BorderWidth(0, 0, 1, 0).BorderColor(p.Border).Children(func() {
				ui.Row(c).FillWidth().MinWidth(0).AlignItems(ui.Center).Gap(8).Children(func() {
					ui.Text(c, item.Task.Name).FontSize(16).FontWeight(600).SingleLine().Grow(1).Shrink(1).MinWidth(0).TextColor(p.Foreground)
					renderTaskStatusBadge(c, item, t)
				})
				ui.Row(c).FillWidth().MinWidth(0).AlignItems(ui.Center).Gap(12).Children(func() {
					ui.Text(c, FormatScheduleSummary(item.Task.Timing, nowMs, ctrl.loc, t)).FontSize(12).Font("monospace").
						SingleLine().Shrink(0).TextColor(p.MutedForeground)
					ui.Row(c).AlignItems(ui.Center).Gap(4).MinWidth(0).Shrink(1).Children(func() {
						ui.Icon(c, icons.Folder).FontSize(12).TextColor(p.MutedForeground).Shrink(0)
						ui.Text(c, projName).FontSize(12).SingleLine().MinWidth(0).Shrink(1).TextColor(p.MutedForeground)
					})
				})
			})

			if tab == "records" {
				ctrl.renderRecordsTab(c, item, runs, loaded, more, loading, busy || hasPending, nowMs, t)
			} else {
				ctrl.renderDetailsTab(c, item, runs, nowMs, projName, wtPath, teamName, hostLabel, multiHost, t)
			}
		})
	})
}

func (ctrl *controller) renderDetailsTab(
	c *ui.Context,
	item TaskItem,
	runs []protocol.ScheduledTaskHistoryEntry,
	nowMs int64,
	projName, wtPath, teamName, hostLabel string,
	multiHost bool,
	t Translate,
) {
	p := theme.Of(c)
	streak := FailureStreak(runs)

	ui.Column(c.Key("overview")).FillWidth().Gap(16).Padding(12, 16, 24, 16).Children(func() {
		// Consecutive failure streak card (when >= 2).
		if streak >= 2 {
			failed := runs[0]
			at := failed.Job.CreatedMs
			if failed.Job.StartedMs != nil {
				at = *failed.Job.StartedMs
			}
			ui.Row(c.Key("failure-streak")).FillWidth().AlignItems(ui.Start).Gap(10).Padding(12).Radius(p.Radius).
				Border(1, p.Danger.Alpha(0.35)).Background(p.Danger.Alpha(0.08)).Children(func() {
				ui.Icon(c, icons.CircleAlert).FontSize(16).TextColor(p.Danger).Shrink(0)
				ui.Column(c).Grow(1).Shrink(1).MinWidth(0).Gap(4).Children(func() {
					ui.Text(c, t("scheduled_tasks_last_failed")).FontSize(14).FontWeight(600).TextColor(p.Foreground)
					if failed.Job.Error != nil && failed.Job.Error.Message != "" {
						ui.Text(c, failed.Job.Error.Message).FontSize(12).TextColor(p.Foreground)
					}
					meta := FormatWhen(at, nowMs, ctrl.loc, t) + " · " + t("scheduled_tasks_failed_streak", "count", streak)
					ui.Text(c, meta).FontSize(12).TextColor(p.MutedForeground)
				})
			})
		}

		// Visual schedule card.
		ctrl.renderScheduleCard(c, item, nowMs, t)

		// Facts list.
		ctrl.renderFacts(c, item, runs, nowMs, projName, wtPath, teamName, hostLabel, multiHost, t)

		// Prompt section with Copy button.
		renderDetailSection(c, "prompt", t("scheduled_tasks_prompt"), func() {
			kit.IconButton(c.Key("copy"), icons.Copy, t("scheduled_tasks_copy"), 24, false).OnClick(func() {
				if ctrl.services.WriteClipboard != nil {
					_ = ctrl.services.WriteClipboard(item.Task.Prompt)
				}
				kit.Copied(c)
			})
		}, func() {
			ui.Box(c.Key("prompt-box")).FillWidth().Radius(p.Radius).Background(p.Muted).Padding(10, 12).Children(func() {
				ui.Text(c, item.Task.Prompt).FontSize(13).MaxLines(5).TextColor(p.Foreground)
			})
		})

		// Last 14 runs bar strip.
		recentCount := min(len(runs), recentStripLimit)
		recent := make([]protocol.ScheduledTaskHistoryEntry, recentCount)
		for i := range recentCount {
			recent[i] = runs[recentCount-1-i]
		}
		renderDetailSection(c, "recent", t("scheduled_tasks_recent", "count", recentStripLimit), func() {
			ui.Text(c, RecentSummaryText(recent, t)).FontSize(12).TextColor(p.MutedForeground)
		}, func() {
			if len(recent) == 0 {
				return
			}
			ui.Row(c.Key("strip")).FillWidth().Gap(4).Children(func() {
				for _, run := range recent {
					col := runStatusColor(run.Job.Status, p)
					bar := ui.Box(c.Key(run.Job.ID.String())).Grow(1).MinWidth(0).Height(22).Radius(4).Background(col)
					if run.Job.Status == protocol.DispatchStatusCancelled {
						bar.Opacity(0.6)
					}
				}
			})
		})

		// Upcoming 5 runs.
		upcomingTimes := Upcoming(item, nowMs, 5)
		renderDetailSection(c, "upcoming", t("scheduled_tasks_upcoming"), nil, func() {
			if !item.Task.Enabled {
				ui.Text(c, t("scheduled_tasks_resume_note")).FontSize(12).TextColor(p.MutedForeground)
				return
			}
			if len(upcomingTimes) == 0 {
				ui.Text(c, t("scheduled_tasks_no_upcoming")).FontSize(12).TextColor(p.MutedForeground)
				return
			}
			ui.Column(c).FillWidth().Gap(4).Children(func() {
				for i, at := range upcomingTimes {
					ui.Row(c.Key(i)).FillWidth().Height(24).AlignItems(ui.Center).Justify(ui.SpaceBetween).Gap(12).Children(func() {
						ui.Text(c, FormatWhen(at, nowMs, ctrl.loc, t)).FontSize(12).Font("monospace").TextColor(p.Foreground)
						ui.Text(c, FormatRelative(at, nowMs, ctrl.loc, t)).FontSize(12).TextColor(p.MutedForeground)
					})
				}
			})
		})
	})
}

func (ctrl *controller) renderScheduleCard(c *ui.Context, item TaskItem, nowMs int64, t Translate) {
	p := theme.Of(c)
	nextList := Upcoming(item, nowMs, 1)
	nextValue := "—"
	if !item.Task.Enabled {
		nextValue = t("scheduled_tasks_paused")
	} else if len(nextList) > 0 {
		nextValue = FormatWhen(nextList[0], nowMs, ctrl.loc, t)
	}

	var hero, side string
	var footerCells [][2]string

	if every, ok := timingEvery(item.Task.Timing); ok {
		hero = FormatInterval(every.IntervalMs, t)
		side = t("scheduled_tasks_fixed_interval")
		footerCells = [][2]string{
			{t("scheduled_tasks_start"), FormatWhen(every.AnchorMs, nowMs, ctrl.loc, t)},
			{t("scheduled_tasks_next"), nextValue},
		}
	} else if weekly, ok := timingWeekly(item.Task.Timing); ok {
		hero = FormatWallClock(weekly.Hour, weekly.Minute)
		if len(weekly.Weekdays) == 1 {
			side = t("scheduled_tasks_weekly_days_one")
		} else {
			side = t("scheduled_tasks_weekly_days_other", "count", len(weekly.Weekdays))
		}
		footerCells = [][2]string{
			{t("scheduled_tasks_timezone"), weekly.Timezone},
			{t("scheduled_tasks_next"), nextValue},
		}
	} else if once, ok := timingOnce(item.Task.Timing); ok {
		hero = FormatClock(once.AtMs, false, ctrl.loc)
		side = t("scheduled_tasks_once")
		footerCells = [][2]string{
			{t("scheduled_tasks_next"), nextValue},
		}
	}

	ui.Column(c.Key("plan-card")).FillWidth().Radius(p.Radius).Border(1, p.Border).Background(p.Muted).Clip().Children(func() {
		// Hero row.
		ui.Row(c).FillWidth().AlignItems(ui.End).Justify(ui.SpaceBetween).Gap(8).Padding(12, 12, 8, 12).Children(func() {
			ui.Text(c, hero).FontSize(22).FontWeight(700).Font("monospace").TextColor(p.Foreground)
			ui.Text(c, side).FontSize(12).SingleLine().TextColor(p.MutedForeground)
		})

		// Mode-specific visual body.
		if weekly, ok := timingWeekly(item.Task.Timing); ok {
			today := WeeklyTodayWeekday(weekly, item.NextMs, nowMs, ctrl.loc)
			ui.Row(c.Key("plan-days")).FillWidth().Gap(4).Padding(0, 12, 12, 12).Children(func() {
				for day := uint8(1); day <= 7; day++ {
					active := slices.Contains([]byte(weekly.Weekdays), day)
					isToday := day == today
					labelColor := p.MutedForeground
					if isToday {
						labelColor = p.Foreground
					}
					ui.Column(c.Key(day)).Grow(1).MinWidth(0).AlignItems(ui.Center).Gap(4).Children(func() {
						ui.Text(c, t(fmt.Sprintf("scheduled_tasks_weekday_%d", day))).FontSize(11).SingleLine().TextColor(labelColor)
						pillBg := p.Background
						if active {
							pillBg = p.Primary.Alpha(0.16)
						}
						pill := ui.Row(c).FillWidth().Height(32).Radius(6).Center().Background(pillBg)
						if isToday {
							pill.Border(1, p.Ring)
						}
						pill.Children(func() {
							if active {
								ui.Box(c).Size(8, 8).Radius(4).Background(p.Primary)
							} else {
								ui.Box(c).Size(8, 2).Radius(1).Background(p.Border)
							}
						})
					})
				}
			})
		} else if every, ok := timingEvery(item.Task.Timing); ok {
			strip := ComputeRibbon(every, nowMs, ctrl.loc, 96)
			ui.Column(c.Key("plan-ribbon")).FillWidth().Gap(6).Padding(0, 12, 12, 12).Children(func() {
				ui.Box(c.Key("band")).FillWidth().Height(30).Radius(6).Background(p.Background).Clip().Draw(func(painter *ui.Painter, bounds ui.Rect) {
					if bounds.W <= 0 || bounds.H <= 0 {
						return
					}
					nowX := bounds.X + float32(strip.Now)*bounds.W
					painter.Fill(ui.Rect{X: bounds.X, Y: bounds.Y, W: max(nowX-bounds.X, 0), H: bounds.H}, p.Primary.Alpha(0.08), 0)
					for _, frac := range strip.Ticks {
						col := p.Primary
						if frac <= strip.Now {
							col = p.Primary.Alpha(0.6)
						}
						tx := bounds.X + float32(frac)*bounds.W
						painter.Fill(ui.Rect{X: tx, Y: bounds.Y + 7, W: 3, H: max(bounds.H-14, 2)}, col, 1)
					}
					painter.Fill(ui.Rect{X: nowX - 1, Y: bounds.Y + 2, W: 2, H: max(bounds.H-4, 2)}, p.Foreground, 1)
				})
				ui.Row(c).FillWidth().Justify(ui.SpaceBetween).Children(func() {
					for _, hour := range []string{"00", "06", "12", "18", "24"} {
						ui.Text(c, hour).FontSize(11).Font("monospace").TextColor(p.MutedForeground)
					}
				})
			})
		} else if once, ok := timingOnce(item.Task.Timing); ok {
			dt := time.UnixMilli(once.AtMs).In(ctrl.loc)
			monthLabel := t(fmt.Sprintf("scheduled_tasks_month_%d", int(dt.Month())))
			weekdayLabel := t(fmt.Sprintf("scheduled_tasks_weekday_%d", isoWeekday(dt.Weekday())))
			ui.Row(c.Key("plan-once")).FillWidth().AlignItems(ui.Center).Gap(12).Padding(0, 12, 12, 12).Children(func() {
				ui.Column(c).Width(46).Shrink(0).Radius(p.Radius).Border(1, p.Border).Background(p.Background).Clip().AlignItems(ui.Center).Children(func() {
					ui.Row(c).FillWidth().Center().Padding(2, 0).Background(p.Primary).Children(func() {
						ui.Text(c, monthLabel).FontSize(11).FontWeight(600).TextColor(p.PrimaryForeground)
					})
					ui.Text(c, strconv.Itoa(dt.Day())).FontSize(16).FontWeight(700).Font("monospace").Padding(4, 0).TextColor(p.Foreground)
				})
				ui.Text(c, weekdayLabel+" "+FormatClock(once.AtMs, false, ctrl.loc)).FontSize(12).TextColor(p.MutedForeground)
			})
		}

		// Footer metadata strip.
		if len(footerCells) > 0 {
			ui.Row(c.Key("plan-footer")).FillWidth().BorderWidth(1, 0, 0, 0).BorderColor(p.Border).Children(func() {
				for i, cell := range footerCells {
					box := ui.Column(c.Key(i)).Grow(1).Shrink(1).MinWidth(0).Gap(2).Padding(8, 12)
					if i > 0 {
						box.BorderWidth(0, 0, 0, 1).BorderColor(p.Border)
					}
					box.Children(func() {
						ui.Text(c, cell[0]).FontSize(11).TextColor(p.MutedForeground)
						ui.Text(c, cell[1]).FontSize(12).Font("monospace").SingleLine().TextColor(p.Foreground)
					})
				}
			})
		}
	})
}

func (ctrl *controller) renderFacts(
	c *ui.Context,
	item TaskItem,
	runs []protocol.ScheduledTaskHistoryEntry,
	nowMs int64,
	projName, wtPath, teamName, hostLabel string,
	multiHost bool,
	t Translate,
) {
	p := theme.Of(c)
	nextList := Upcoming(item, nowMs, 1)
	nextVal := "—"
	nextNote := ""
	if !item.Task.Enabled {
		nextVal = t("scheduled_tasks_paused")
	} else if len(nextList) > 0 {
		nextVal = FormatWhen(nextList[0], nowMs, ctrl.loc, t)
		nextNote = FormatRelative(nextList[0], nowMs, ctrl.loc, t)
	}

	lastVal := t("scheduled_tasks_never_run")
	lastNote := ""
	var lastDot *ui.Color
	if len(runs) > 0 {
		at := runs[0].Job.CreatedMs
		if runs[0].Job.StartedMs != nil {
			at = *runs[0].Job.StartedMs
		}
		lastVal = FormatWhen(at, nowMs, ctrl.loc, t)
		lastNote = t(RunLabelKey(runs[0].Job.Status))
		col := runStatusColor(runs[0].Job.Status, p)
		lastDot = &col
	}

	ui.Column(c.Key("facts")).FillWidth().Gap(2).Padding(12, 0, 0, 0).BorderWidth(1, 0, 0, 0).BorderColor(p.Border).Children(func() {
		renderFactRow(c, "next", t("scheduled_tasks_next_run"), nextVal, nextNote, nil, nil)
		renderFactRow(c, "last", t("scheduled_tasks_last_run"), lastVal, lastNote, lastDot, nil)
		renderFactRow(c, "project", t("scheduled_tasks_project"), projName, "", nil, icons.Folder)
		if item.Task.Project != nil {
			renderFactRow(c, "worktree", t("scheduled_tasks_worktree"), wtPath, "", nil, nil)
		}
		if multiHost {
			renderFactRow(c, "host", t("scheduled_tasks_execution_host"), hostLabel, "", nil, nil)
		}
		if item.Task.Config.Model != "" {
			modelText := item.Task.Config.Model
			eff := effortString(item.Task.Config.Effort)
			if eff != "" && eff != "default" {
				modelText += " " + eff
			}
			renderFactRow(c, "model", t("scheduled_tasks_model"), modelText, "", nil, nil)
		}
		execMode := item.Task.Execution
		if execMode == "" {
			execMode = protocol.ConversationExecutionTeam
		}
		execText := teamName
		if execMode == protocol.ConversationExecutionModel {
			execText = t("scheduled_tasks_model")
		}
		renderFactRow(c, "execution", t("scheduled_tasks_execution"), execText, "", nil, nil)
		statusText := t("scheduled_tasks_paused")
		statusDot := p.MutedForeground
		if item.Task.Enabled {
			statusText = t("scheduled_tasks_enabled")
			statusDot = p.Success
		}
		renderFactRow(c, "status", t("scheduled_tasks_status"), statusText, "", &statusDot, nil)
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

func renderDetailSection(c *ui.Context, key, title string, trailing, body func()) {
	p := theme.Of(c)
	ui.Column(c.Key(key)).FillWidth().Gap(8).Padding(12, 0, 0, 0).BorderWidth(1, 0, 0, 0).BorderColor(p.Border).Children(func() {
		ui.Row(c).FillWidth().AlignItems(ui.Center).Justify(ui.SpaceBetween).Gap(8).Children(func() {
			ui.Text(c, title).FontSize(12).FontWeight(600).TextColor(p.MutedForeground)
			if trailing != nil {
				trailing()
			}
		})
		if body != nil {
			body()
		}
	})
}

func (ctrl *controller) renderRecordsTab(
	c *ui.Context,
	item TaskItem,
	runs []protocol.ScheduledTaskHistoryEntry,
	loaded, more, loading, disabled bool,
	nowMs int64,
	t Translate,
) {
	p := theme.Of(c)
	if !loaded {
		ui.Row(c.Key("records-loading")).FillWidth().Height(96).Center().Children(func() {
			kit.Loader(c, 22)
		})
		return
	}
	if len(runs) == 0 {
		ui.Box(c.Key("records-empty")).FillWidth().Padding(12).Children(func() {
			kit.Result(c, kit.ResultProps{Icon: icons.Calendar, Title: t("scheduled_tasks_no_runs")})
		})
		return
	}

	fallbackNode := ctrl.executionNodeOf(item.Task)
	ui.Column(c.Key("records-timeline")).FillWidth().Padding(12, 12, 24, 12).Children(func() {
		lastDay := ""
		for i, run := range runs {
			at := run.Job.CreatedMs
			if run.Job.StartedMs != nil {
				at = *run.Job.StartedMs
			}
			day := FormatDayLabel(at, nowMs, ctrl.loc, t)
			if day != lastDay {
				lastDay = day
				renderTimelineRail(c, fmt.Sprintf("day-%d", i), false, 10, func() {
					ui.Box(c).Margin(4, 0, 0, 0).Size(8, 8).Shrink(0).Radius(4).Border(2, p.Border).Background(p.Background)
				}, func() {
					ui.Text(c, day).FontSize(12).FontWeight(600).TextColor(p.MutedForeground)
				})
			}

			run := run
			isLast := i == len(runs)-1 && !more
			failed := run.Job.Status == protocol.DispatchStatusFailed
			stoppable := run.Job.Status == protocol.DispatchStatusQueued || (run.Job.Status == protocol.DispatchStatusRunning && run.Turn != nil)
			renderTimelineRail(c, "run-"+run.Job.ID.String(), isLast, 12, func() {
				ui.Box(c).Margin(11, 0, 0, 0).Size(12, 12).Shrink(0).Radius(6).Background(runStatusColor(run.Job.Status, p))
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
						ui.Text(c, t(RunLabelKey(run.Job.Status))).FontSize(13).FontWeight(500).TextColor(titleCol)
						ui.Text(c, FormatClock(at, true, ctrl.loc)).FontSize(12).Font("monospace").TextColor(p.MutedForeground)
						ui.Spacer(c)
						if stoppable {
							label := t("scheduled_tasks_stop")
							if run.Job.Status == protocol.DispatchStatusQueued {
								label = t("scheduled_tasks_cancel")
							}
							kit.IconButton(c.Key("stop"), icons.Square, label, 24, false).Disabled(disabled).OnClick(func() {
								ctrl.cancelOrStopRun(run, fallbackNode)
							})
						}
						if run.Session != nil {
							sess := *run.Session
							targetNode := run.ExecutionNode
							if targetNode == (protocol.NodeID{}) {
								targetNode = fallbackNode
							}
							kit.IconButton(c.Key("open"), icons.ExternalLink, t("scheduled_tasks_open"), 24, false).OnClick(func() {
								if ctrl.services.OpenSession != nil {
									ctrl.services.OpenSession(targetNode, sess)
								}
							})
						}
					})
					if run.Job.Error != nil && run.Job.Error.Message != "" {
						errCol := p.MutedForeground
						if failed {
							errCol = p.Danger
						}
						ui.Text(c, run.Job.Error.Message).FontSize(12).TextColor(errCol)
					}
				})
			})
		}
		if more {
			ui.Row(c.Key("load-more")).FillWidth().Justify(ui.Center).Padding(8, 0, 0, 0).Children(func() {
				kit.SecondaryBusy(c.Key("btn"), t("scheduled_tasks_load_more"), loading).OnClick(ctrl.loadMoreHistory)
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

func runStatusColor(status protocol.DispatchStatus, p *theme.Palette) ui.Color {
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
