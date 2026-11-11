package shell

import (
	"fmt"
	"slices"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// NoticeHeading is the heading a package gave its notice, in a language, ""
// when it gave none: a reminder says Reminder above its own title. System
// notices keep the application's headings.
func NoticeHeading(n *viewmodel.Notice, language string) string {
	if n.Presentation == nil {
		return ""
	}
	if n.Details != nil {
		switch n.Details.Kind {
		case "power", "diagnostic", "system":
			return ""
		}
	}
	if heading := n.Presentation.Locales[language]; heading != "" {
		return heading
	}
	return n.Presentation.Heading
}

// noticeTitle names a notice in the bell and the centre: its own title, and a
// notice without one by its package's heading before the generic fallback.
func noticeTitle(c *ui.Context, n *viewmodel.Notice) string {
	if strings.TrimSpace(n.Title) == "" {
		if heading := NoticeHeading(n, i18n.Match(c.Locale())); heading != "" {
			return heading
		}
	}
	if n.TitleKey != "" {
		return i18n.T(c, n.TitleKey)
	}
	return n.Title
}

// noticeSource names the host a notice came from.
func (a *App) noticeSource(c *ui.Context, n *viewmodel.Notice) string {
	if a.Data.Workspace != nil {
		for _, host := range a.Data.Workspace.Hosts {
			if host.ID == n.Node {
				return a.hostName(c, host.Name)
			}
		}
	}
	return a.hostName(c, n.Host)
}

func (a *App) noticeIDs(read bool) []string {
	var ids []string
	for _, n := range a.Data.Notices {
		if n.Read == read {
			ids = append(ids, n.ID)
		}
	}
	return ids
}

// notificationCenter uses the same cards and notice presentation as the bell.
func (a *App) notificationCenter(c *ui.Context) {
	kit.Page(c, "notification-center", i18n.T(c, "notification_center"), "", func() {
		ui.Row(c.Key("toolbar")).Gap(8).Children(func() {
			kit.Segmented(c, "filter", &a.noticeTab, []string{i18n.T(c, "notification_all"), i18n.T(c, "notification_unread")}, kit.SegmentedProps{})
			ui.Spacer(c)
			kit.Secondary(c.Key("read"), i18n.T(c, "notifications_read_all")).Disabled(len(a.noticeIDs(false)) == 0).OnClick(func() {
				if a.Hooks.ReadNotices != nil {
					a.Hooks.ReadNotices(a.noticeIDs(false))
					return
				}
				for _, n := range a.Data.Notices { // the preview, as the bell's
					n.Read = true
				}
			})
			kit.Secondary(c.Key("delete"), i18n.T(c, "notification_delete_read")).Disabled(len(a.noticeIDs(true)) == 0).OnClick(func() {
				if a.Hooks.DismissRead != nil {
					a.Hooks.DismissRead(a.noticeIDs(true))
					return
				}
				a.Data.Notices = slices.DeleteFunc(a.Data.Notices, func(n *preview.Notice) bool { return n.Read })
			})
		})
		shown := 0
		for _, n := range SortNotices(a.Data.Notices) {
			expanded := a.noticeDetail != nil && a.noticeDetail.ID == n.ID
			if a.noticeTab == 1 && n.Read && !expanded {
				continue
			}
			shown++
			kit.Card(c.Key(n.ID), func() {
				p := theme.Of(c)
				glyph, color, _ := noticeLook(p, n.Kind)
				ui.Row(c.Key("heading")).Padding(12).Gap(8).Children(func() {
					ui.Icon(c, glyph).FontSize(16).TextColor(color)
					b := kit.Ghost(c.Key("details"), noticeTitle(c, n), expanded).Grow(1).Justify(ui.Start).Children(func() {
						text := ui.Text(c, noticeTitle(c, n)).FontSize(14).SingleLine()
						if !n.Read {
							text.FontWeight(500)
						}
					}).OnClick(func() {
						if expanded {
							a.noticeDetail = nil
						} else {
							a.noticeDetail = n
							if a.Hooks.ReadNotice != nil {
								a.Hooks.ReadNotice(n.ID)
							}
						}
					})
					_ = b
					if n.Session != "" && a.Data.Find(n.Session) != nil {
						kit.IconButton(c.Key("open"), icons.ChevronRight, i18n.T(c, "notification_open_conversation"), 28, false).OnClick(func() {
							if a.Hooks.ReadNotice != nil {
								a.Hooks.ReadNotice(n.ID)
							}
							a.selectConversation(n.Session)
						})
					}
					kit.IconButton(c.Key("dismiss"), icons.Trash, i18n.T(c, "settings_delete"), 28, false).OnClick(func() {
						if a.Hooks.DismissNotices != nil {
							a.Hooks.DismissNotices([]string{n.ID})
						}
					})
				})
				if expanded {
					a.notificationDetails(c, n)
				}
			})
		}
		if shown == 0 {
			kit.Result(c, kit.ResultProps{Icon: icons.Bell, Title: i18n.T(c, "notifications_empty"), Card: true})
		}
	})
}

func (a *App) notificationDetails(c *ui.Context, n *viewmodel.Notice) {
	p := theme.Of(c)
	ui.Column(c.Key("body")).Padding(12).Gap(12).BorderWidth(1, 0, 0, 0).BorderColor(p.Border).Children(func() {
		for _, text := range []string{n.Message, n.DetailText} {
			if text != "" {
				ui.Text(c, text).FontSize(14).Selectable()
			}
		}
		facts := [][2]string{{i18n.T(c, "notification_source"), a.noticeSource(c, n)}}
		if !n.Time.IsZero() {
			facts = append(facts, [2]string{i18n.T(c, "notification_time"), n.Time.Local().Format("2006-01-02 15:04:05")})
		}
		if n.Session != "" {
			title := n.Session
			if cv := a.Data.Find(n.Session); cv != nil {
				title = cv.Title
			}
			facts = append(facts, [2]string{i18n.T(c, "settings_conversation"), title})
		}
		if n.Details != nil {
			switch value := n.Details.Data.(type) {
			case *protocol.NotificationDetailsTask:
				facts = append(facts, [2]string{i18n.T(c, "notification_result"), i18n.T(c, statusKey(value.Status))})
				if value.StartedMs != nil && value.FinishedMs != nil {
					facts = append(facts, [2]string{i18n.T(c, "notification_duration"), i18n.T(c, "notification_seconds", "count", max(0, (*value.FinishedMs-*value.StartedMs)/int64(time.Second/time.Millisecond)))})
				}
			case *protocol.NotificationDetailsPowerPayload:
				facts = append(facts, [2]string{i18n.T(c, "notification_result"), fmt.Sprint(value.Code)})
			}
		}
		for _, fact := range facts {
			kit.Record(c, fact[0], fact[1])
		}
		if n.Report != "" {
			kit.Secondary(c.Key("copy"), i18n.T(c, "notification_copy_report")).OnClick(func() { c.WriteClipboard(n.Report); kit.Toast(c, kit.Done, i18n.T(c, "notification_report_copied")) })
			ui.Text(c, n.Report).FontSize(12).Selectable()
		}
	})
}

func statusKey(status protocol.ConversationStatus) string {
	switch status {
	case protocol.ConversationStatusQueued:
		return "turn_queued"
	case protocol.ConversationStatusRunning:
		return "activity_running"
	case protocol.ConversationStatusStopping:
		return "chat_stopping"
	case protocol.ConversationStatusCompleted:
		return "activity_completed"
	case protocol.ConversationStatusContinued:
		return "turn_continued"
	case protocol.ConversationStatusCancelled:
		return "turn_cancelled"
	case protocol.ConversationStatusInterrupted:
		return "chat_interrupted"
	default:
		return "activity_failed"
	}
}
