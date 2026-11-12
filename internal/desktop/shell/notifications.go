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

// notificationCenter is the bell's notices as one card of rows, newest
// first, under the All and Unread filter and the bell's two actions.
func (a *App) notificationCenter(c *ui.Context) {
	kit.Page(c, "notification-center", i18n.T(c, "notification_center"), "", func() {
		ui.Row(c.Key("toolbar")).Gap(8).Children(func() {
			kit.Segmented(c, "filter", &a.noticeTab, []string{i18n.T(c, "notification_all"), i18n.T(c, "notification_unread")}, kit.SegmentedProps{})
			ui.Spacer(c)
			kit.Secondary(c.Key("read"), i18n.T(c, "notifications_read_all")).Disabled(len(a.noticeIDs(false)) == 0).OnClick(func() {
				a.readNotices()
			})
			kit.Secondary(c.Key("delete"), i18n.T(c, "notification_delete_read")).Disabled(len(a.noticeIDs(true)) == 0).OnClick(func() {
				a.dismissRead()
			})
		})
		var shown []*viewmodel.Notice
		for _, n := range SortNotices(a.Data.Notices) {
			// The notice open stays under Unread though opening it read it.
			if a.noticeTab == 1 && n.Read && a.noticeDetail != n {
				continue
			}
			shown = append(shown, n)
		}
		if len(shown) == 0 {
			kit.Result(c, kit.ResultProps{Icon: icons.Bell, Title: i18n.T(c, "notifications_empty"), Card: true})
			return
		}
		kit.Card(c.Key("list"), func() {
			for _, n := range shown {
				kit.Bleed(c, n.ID, func() {
					expanded := a.noticeDetail == n
					a.notificationRow(c, n, expanded)
					if expanded {
						a.notificationDetails(c, n)
					}
				})
			}
		})
	})
}

// notificationRow is a notice in the centre's card, at the row metrics: the
// kind's glyph in the lead slot, the title (medium while unread) over a
// muted line of its kind, host and age, then the unread dot and the quiet
// actions. The whole row is the button that opens the notice's details and
// fills to the card's edges under the pointer.
func (a *App) notificationRow(c *ui.Context, n *viewmodel.Notice, expanded bool) {
	p := theme.Of(c)
	title := noticeTitle(c, n)
	glyph, color, kind := noticeLook(p, n.Kind)
	var facts []string
	if n.Kind != preview.NoticeInfo {
		facts = append(facts, i18n.T(c, kind))
	}
	facts = append(facts, a.noticeSource(c, n))
	if !n.Time.IsZero() {
		facts = append(facts, a.noticeAge(c, n))
	}
	row := ui.ButtonBase(c.Key("row")).Label(title).Expanded(expanded).Padding(kit.RowPad, 16).Margin(0, -16).Gap(kit.RowIconGap).
		MinHeight(kit.RowMinHeight).Justify(ui.Start).Cursor(ui.CursorPointer).TextColor(p.SurfaceForeground)
	if row.Hovered() {
		row.Background(p.Hover)
	}
	row.Children(func() {
		ui.Row(c.Key("lead")).Size(kit.RowIcon, kit.RowIcon).Shrink(0).Center().Children(func() {
			ui.Icon(c, glyph).FontSize(18).TextColor(color)
		})
		ui.Column(c.Key("text")).Grow(1).Shrink(1).MinWidth(0).Gap(kit.TitleGap).Children(func() {
			text := ui.Text(c, title).FontSize(kit.RowFont).SingleLine()
			if !n.Read {
				text.FontWeight(500)
			}
			ui.Text(c, strings.Join(facts, " · ")).FontSize(kit.RowFont).SingleLine().TextColor(p.Description())
		})
		ui.Row(c.Key("actions")).Shrink(0).Gap(4).Children(func() {
			// The dot's room stays when read, so the actions line up.
			dot := ui.Box(c).Size(6, 6).Radius(3).Shrink(0).Margin(0, 6)
			if !n.Read {
				dot.Background(p.Primary)
			}
			if n.Session != "" && a.Data.Find(n.Session) != nil {
				kit.IconButton(c.Key("open"), icons.ChevronRight, i18n.T(c, "notification_open_conversation"), 28, false).TextColor(p.MutedForeground).OnClick(func() {
					a.readNotice(n)
					a.selectConversation(n.Session)
				})
			}
			kit.IconButton(c.Key("dismiss"), icons.Trash, i18n.T(c, "settings_delete"), 28, false).TextColor(p.MutedForeground).OnClick(func() {
				a.dismissNotice(n)
			})
		})
	}).OnClick(func() {
		if expanded {
			a.noticeDetail = nil
			return
		}
		a.noticeDetail = n
		a.readNotice(n)
	})
}

// notificationDetails is an open notice's text, facts and report under its
// row, in line with the row's title.
func (a *App) notificationDetails(c *ui.Context, n *viewmodel.Notice) {
	ui.Column(c.Key("body")).Padding(0, 0, kit.RowPad, kit.RowIcon+kit.RowIconGap).Gap(12).Children(func() {
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

// The notices' actions go to the Node through the window's hooks. Where
// none is set, and in the demo, whose notices are its own and reach no
// Node, they change the list in memory.

func (a *App) readNotice(n *preview.Notice) {
	if !a.Demo() && a.Hooks.ReadNotice != nil {
		a.Hooks.ReadNotice(n.ID)
		return
	}
	n.Read = true
}

func (a *App) readNotices() {
	if !a.Demo() && a.Hooks.ReadNotices != nil {
		a.Hooks.ReadNotices(a.noticeIDs(false))
		return
	}
	for _, n := range a.Data.Notices {
		n.Read = true
	}
}

func (a *App) dismissRead() {
	if !a.Demo() && a.Hooks.DismissRead != nil {
		a.Hooks.DismissRead(a.noticeIDs(true))
		return
	}
	a.Data.Notices = slices.DeleteFunc(a.Data.Notices, func(n *preview.Notice) bool { return n.Read })
}

func (a *App) dismissNotice(n *preview.Notice) {
	if !a.Demo() && a.Hooks.DismissNotices != nil {
		a.Hooks.DismissNotices([]string{n.ID})
		return
	}
	a.Data.Notices = slices.DeleteFunc(a.Data.Notices, func(x *preview.Notice) bool { return x == n })
	if a.noticeDetail == n {
		a.noticeDetail = nil
	}
}
