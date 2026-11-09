package shell

import (
	"slices"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/preview"
	"github.com/sailry/sailry-go/internal/sidebar"
	"github.com/sailry/sailry-go/internal/theme"
)

// NoticeAge is a notice's age as the activity centre writes it: an i18n
// key and its count (notifications.rs).
func NoticeAge(d time.Duration) (key string, count int) {
	switch {
	case d < time.Minute:
		return "notifications_time_now", 0
	case d < time.Hour:
		return "notifications_time_minutes", int(d / time.Minute)
	case d < 24*time.Hour:
		return "notifications_time_hours", int(d / time.Hour)
	}
	return "notifications_time_days", int(d / (24 * time.Hour))
}

// SortNotices orders notices newest first.
func SortNotices(list []*preview.Notice) []*preview.Notice {
	out := slices.Clone(list)
	slices.SortStableFunc(out, func(a, b *preview.Notice) int { return b.Time.Compare(a.Time) })
	return out
}

// noticeLook is a kind's glyph, its color and label (activity/icons.rs).
func noticeLook(p *theme.Palette, k preview.NoticeKind) (*ui.SVG, ui.Color, string) {
	switch k {
	case preview.NoticeCompleted:
		return icons.CircleCheck, p.Success, "activity_completed"
	case preview.NoticeFailed:
		return icons.CircleX, p.Danger, "activity_failed"
	case preview.NoticeApproval:
		return icons.ShieldAlert, p.Warning, "activity_approval"
	case preview.NoticeInput:
		return icons.Question, p.Warning, "activity_input"
	}
	return icons.Info, p.MutedForeground, "notifications"
}

// bell is the activity centre (activity/notifications.rs): a bell with a
// dot while notices are unread, opening a frosted popover to its right
// with Mark all as read, Delete read and the notices, newest first.
func (a *App) bell(c *ui.Context) {
	p := theme.Of(c)
	unread := 0
	for _, n := range a.Data.Notices {
		if !n.Read {
			unread++
		}
	}
	label := i18n.T(c, "notifications")
	b := kit.IconButton(c.Key("bell"), icons.Bell, label, 36, a.activity).OnClick(func() { a.activity = !a.activity })
	b.HasPopup(ui.PopupDialog).Expanded(a.activity)
	if unread > 0 {
		b.DrawOver(func(pt *ui.Painter, r ui.Rect) {
			pt.Fill(ui.Rect{X: r.X + r.W - 12, Y: r.Y + 6, W: 6, H: 6}, p.Danger, 3)
		})
	}
	ui.PopoverBase(c, b, &a.activity, func(panel ui.Element) {
		w, h := c.Size()
		kit.Floating(c, panel).AttachTo(b, ui.AnchorRight, ui.AnchorLeft).MarginStart(8).Width(min(320, w-32)).Padding(8).Gap(4)
		ui.Row(c.Key("head")).Height(40).PaddingStart(8).Gap(4).Children(func() {
			center := i18n.T(c, "notification_center")
			kit.Ghost(c.Key("center"), center, false).Height(28).PaddingX(4).Gap(4).Children(func() {
				ui.Text(c, center).FontSize(14).FontWeight(500)
				ui.Icon(c, icons.ChevronRight).FontSize(14).TextColor(p.MutedForeground)
			}).OnClick(func() { c.Toast(i18n.T(c, "preview_notice")) })
			ui.Spacer(c)
			kit.IconButton(c.Key("read"), icons.CheckCheck, i18n.T(c, "notifications_read_all"), 28, false).
				Disabled(unread == 0).OnClick(func() {
				for _, n := range a.Data.Notices {
					n.Read = true
				}
			})
			read := len(a.Data.Notices) - unread
			kit.IconButton(c.Key("delete"), icons.Trash, i18n.T(c, "notification_delete_read"), 28, false).
				Disabled(read == 0).OnClick(func() {
				a.Data.Notices = slices.DeleteFunc(a.Data.Notices, func(n *preview.Notice) bool { return n.Read })
			})
		})
		ui.Scroll(c.Key("list")).MaxHeight(min(h-160, 360)).Gap(2).Children(func() {
			if len(a.Data.Notices) == 0 {
				kit.Empty(c, icons.Bell, i18n.T(c, "notifications_empty"), "", nil)
				return
			}
			for _, n := range SortNotices(a.Data.Notices) {
				a.noticeRow(c, n)
			}
		})
	})
}

// noticeRow is a 40 DIP row: the kind's glyph, the title (medium while
// unread), the age and the unread dot. A click reads it and opens what it
// is about.
func (a *App) noticeRow(c *ui.Context, n *preview.Notice) {
	p := theme.Of(c)
	glyph, color, kind := noticeLook(p, n.Kind)
	key, count := NoticeAge(preview.Now.Sub(n.Time))
	age := i18n.T(c, key, "count", count)
	row := ui.ButtonBase(c.Key(n.ID)).Height(40).PaddingX(8).Gap(8).Radius(p.Radius).Cursor(ui.CursorPointer).
		Label(n.Title + " · " + i18n.T(c, n.Host) + " · " + i18n.T(c, kind) + " · " + n.Message)
	text := p.PopoverForeground
	if n.Read {
		text = p.MutedForeground
	}
	if row.Hovered() {
		row.Background(p.Hover)
	}
	kit.Tip(c, row, n.Title).TextColor(text).Children(func() {
		ui.Icon(c, glyph).FontSize(16).TextColor(color).Shrink(0)
		t := ui.Text(c, n.Title).FontSize(14).SingleLine().Grow(1).Shrink(1)
		if !n.Read {
			t.FontWeight(500)
		}
		ui.Text(c, age).FontSize(12).SingleLine().TextColor(p.MutedForeground).Shrink(0)
		if !n.Read {
			ui.Box(c).Size(6, 6).Radius(3).Background(p.Primary).Shrink(0)
		}
	}).OnClick(func() {
		n.Read = true
		a.activity = false
		switch {
		case n.Link != "":
			a.openKnowledge(c, n.Link)
		case n.Session != "" && a.Data.Find(n.Session) != nil:
			a.Page, a.Sidebar.Selected = ConversationPage, n.Session
		}
	})
}

// lane is where a session shows in the header's strip.
type lane int

const (
	laneWaiting lane = iota
	laneRunning
	laneCompleted
)

// strip is the header's activity strip (shell/activity/strip.rs): the
// sessions waiting, running and finished unread, a 20 DIP identicon each
// in groups, up to three a group (two under 480 DIPs) then +N; the
// pointer resting on one shows its title and project.
func (a *App) strip(c *ui.Context) {
	p := theme.Of(c)
	groups := map[lane][]*preview.Conversation{}
	for _, pr := range a.Data.Projects {
		for _, cv := range pr.Conversations {
			switch {
			case cv.Archived:
			case cv.Status == preview.Waiting:
				groups[laneWaiting] = append(groups[laneWaiting], cv)
			case cv.Status == preview.Running:
				groups[laneRunning] = append(groups[laneRunning], cv)
			case cv.Status == preview.Unread:
				groups[laneCompleted] = append(groups[laneCompleted], cv)
			}
		}
	}
	w, _ := c.Size()
	slots := 3
	if w < 480 {
		slots = 2
	}
	ui.Row(c.Key("strip")).Gap(12).Children(func() {
		for _, l := range []lane{laneWaiting, laneRunning, laneCompleted} {
			list := groups[l]
			if len(list) == 0 {
				continue
			}
			ui.Row(c.Key(int(l))).Gap(4).Children(func() {
				shown := list
				if len(list) > slots {
					shown = list[:slots-1]
				}
				for _, cv := range shown {
					a.avatar(c, cv, l)
				}
				if rest := len(list) - len(shown); rest > 0 {
					color := p.Primary
					switch l {
					case laneWaiting:
						color = p.Warning
					case laneCompleted:
						color = p.Success
					}
					kit.Ghost(c.Key("rest"), "", false).Height(28).PaddingX(6).Children(func() {
						ui.Textf(c, "+%d", rest).FontSize(12).TextColor(color)
					}).OnClick(func() { a.Page, a.Sidebar.Selected = ConversationPage, list[len(shown)].ID })
				}
			})
		}
	})
}

func (a *App) avatar(c *ui.Context, cv *preview.Conversation, l lane) {
	p := theme.Of(c)
	title := sidebar.Title(c, cv)
	status := map[lane]string{laneWaiting: "activity_waiting", laneRunning: "activity_running", laneCompleted: "activity_completed"}[l]
	b := kit.Ghost(c.Key(cv.ID), title+" · "+i18n.T(c, status), cv.ID == a.Sidebar.Selected).Size(28, 28)
	b.Children(func() {
		kit.Identicon(c, cv.ID, l == laneRunning, 20)
		badge, color := icons.TriangleAlert, p.Warning
		switch l {
		case laneRunning:
			return
		case laneCompleted:
			badge, color = icons.Check, p.Success
		}
		ui.Icon(c, badge).FontSize(10).TextColor(color).Absolute().Right(1).Bottom(1)
	}).OnClick(func() { a.Page, a.Sidebar.Selected = ConversationPage, cv.ID })
	project := i18n.T(c, "activity_no_project")
	if pr := a.Data.ProjectOf(cv.ID); pr != nil {
		project = pr.Name
	}
	ui.TooltipBase(c, b, func(tip ui.Element) {
		kit.Floating(c, tip).AttachTo(b, ui.AnchorBottomEnd, ui.AnchorTopEnd).Margin(8, 0, 0, 0).Width(280).Padding(10).Gap(2)
		tip.Children(func() {
			ui.Text(c, title).FontSize(14).FontWeight(600).MaxLines(2)
			ui.Text(c, project).FontSize(12).SingleLine().TextColor(p.MutedForeground)
		})
	})
}
