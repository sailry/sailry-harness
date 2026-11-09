package shell

import (
	"fmt"
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
// dot while notices are unread, opening a popover to its right
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
	w, h := c.Size()
	panel := ui.Popover(c, b, &a.activity, func() {
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
	if panel != (ui.Element{}) {
		panel.AttachTo(b, ui.AnchorRight, ui.AnchorLeft).MarginStart(8).Width(min(320, w-32)).Gap(4)
	}
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
	row.Tooltip(n.Title).TextColor(text).Children(func() {
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

// lane is the group a session shows in on the header's strip.
type lane int

const (
	laneWaiting lane = iota
	laneRunning
	laneCompleted
)

// key is the lane's name: its tooltip and its status.
func (l lane) key() string {
	return [...]string{"activity_waiting", "activity_running", "activity_completed"}[l]
}

// look is the lane's badge glyph and its color, and the progress glyph
// that marks it in lists (activity/icons.rs).
func (l lane) look(p *theme.Palette) (badge, glyph *ui.SVG, color ui.Color) {
	switch l {
	case laneWaiting:
		return icons.TriangleAlert, icons.ProgressWaiting, p.Warning
	case laneCompleted:
		return icons.Check, icons.ProgressCompleted, p.Success
	}
	return nil, icons.ProgressRunning, p.Primary
}

// strip is the header's activity strip (shell/activity/strip.rs): the
// sessions waiting, running and finished unread, in that order, as groups
// 12 DIPs apart of 28 DIP buttons 4 apart. A group shows up to three (two
// under 480 DIPs), or one fewer and +N opening the rest. The pointer
// resting on one shows its title and project.
func (a *App) strip(c *ui.Context) {
	groups := map[lane][]*preview.Conversation{}
	add := func(list []*preview.Conversation) {
		for _, cv := range list {
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
	for _, pr := range a.Data.Projects {
		add(pr.Conversations)
	}
	// Chats without a project show too, their preview naming none.
	add(a.Data.Unassigned)
	w, _ := c.Size()
	slots := 3
	if w < 480 {
		// Keep the header's tools reachable in a narrow window.
		slots = 2
	}
	ui.Row(c.Key("strip")).Gap(12).Shrink(0).Children(func() {
		for _, l := range []lane{laneWaiting, laneRunning, laneCompleted} {
			list := groups[l]
			if len(list) == 0 {
				continue
			}
			ui.Row(c.Key(int(l))).Gap(4).Shrink(0).Children(func() {
				shown := list
				if len(list) > slots {
					shown = list[:slots-1]
				}
				for _, cv := range shown {
					a.avatar(c, cv, l)
				}
				if rest := len(list) - len(shown); rest > 0 {
					a.overflow(c, l, list, rest)
				}
			})
		}
	})
}

// avatar is a session's 28 DIP text button (Kit's text variant: no fill,
// no selected state) holding its 20 DIP agent mark: turning in its color
// while it runs, otherwise in the lane's color with Kit's small icon Badge,
// a 10 DIP disc with a 1 DIP background ring and an 8 DIP glyph at its
// bottom right.
func (a *App) avatar(c *ui.Context, cv *preview.Conversation, l lane) {
	p := theme.Of(c)
	title := sidebar.Title(c, cv)
	b := ui.ButtonBase(c.Key(cv.ID)).Label(title+" · "+i18n.T(c, l.key())).Size(28, 28).Radius(p.Radius).Center().Shrink(0).Cursor(ui.CursorPointer)
	b.Children(func() {
		badge, _, color := l.look(p)
		if badge == nil {
			kit.Agent(c, cv.ID, kit.AgentColor(c, cv.ID), true, 20)
			return
		}
		ui.Box(c).Size(20, 20).Shrink(0).Children(func() {
			kit.Agent(c, cv.ID, color, false, 20)
			ui.Row(c).Absolute().Right(0).Bottom(0).Size(10, 10).Radius(5).Center().
				Background(color).Border(1, p.Background).Children(func() {
				ui.Icon(c, badge).FontSize(8).TextColor(theme.OnTint)
			})
		})
	}).OnClick(func() { a.Page, a.Sidebar.Selected = ConversationPage, cv.ID })
	a.preview(c, b, cv)
}

// overflow is a group's +N: a 28 DIP text button in the lane's color
// (12 DIP), its name as the tooltip, opening a 300 DIP popover below it
// that lists the group's sessions as 36 DIP rows.
func (a *App) overflow(c *ui.Context, l lane, list []*preview.Conversation, rest int) {
	p := theme.Of(c)
	_, _, color := l.look(p)
	if l == laneRunning {
		color = p.Foreground.Alpha(0.9)
	}
	name := i18n.T(c, l.key())
	holder := ui.Box(c.Key("more")).Size(28, 28).Shrink(0)
	open := ui.Local(holder, "open", func() bool { return false })
	holder.Children(func() {
		b := ui.ButtonBase(c.Key("button")).Label(fmt.Sprintf("%s · +%d", name, rest)).Size(28, 28).
			Radius(p.Radius).Center().Cursor(ui.CursorPointer).OnClick(func() { *open = !*open })
		b.HasPopup(ui.PopupDialog).Expanded(*open)
		b.Tooltip(name).Children(func() {
			ui.Textf(c, "+%d", rest).FontSize(12).TextColor(color)
		})
		_, h := c.Size()
		panel := ui.Popover(c, b, open, func() {
			ui.Scroll(c.Key("list")).MaxHeight(min(320, h-96)).Gap(4).Children(func() {
				for _, cv := range list {
					a.overflowRow(c, cv, l, open)
				}
			})
		})
		if panel != (ui.Element{}) {
			panel.AttachTo(b, ui.AnchorBottomEnd, ui.AnchorTopEnd).Margin(4, 0, 0, 0).Width(300)
		}
	})
}

// overflowRow is a session in a +N popover: a ghost row with its 16 DIP
// agent mark, its title and the lane's 16 DIP progress glyph.
func (a *App) overflowRow(c *ui.Context, cv *preview.Conversation, l lane, open *bool) {
	p := theme.Of(c)
	_, glyph, color := l.look(p)
	title := sidebar.Title(c, cv)
	row := kit.Ghost(c.Key(cv.ID), title+" · "+i18n.T(c, l.key()), false).Height(36).PaddingX(8).Gap(8).Shrink(0).Justify(ui.Start)
	row.Children(func() {
		kit.Agent(c, cv.ID, kit.AgentColor(c, cv.ID), false, 16)
		ui.Text(c, title).FontSize(14).SingleLine().Grow(1).Shrink(1).MinWidth(0)
		ui.Icon(c, glyph).FontSize(16).TextColor(color).Shrink(0)
	}).OnClick(func() {
		*open = false
		a.Page, a.Sidebar.Selected = ConversationPage, cv.ID
	})
	a.preview(c, row, cv)
}

// preview is a session's hover card (ui/session_preview.rs): MyGo has no
// styled hover card, so it is the styled tooltip of its title over its
// project.
func (a *App) preview(c *ui.Context, anchor ui.Element, cv *preview.Conversation) {
	project := i18n.T(c, "activity_no_project")
	if pr := a.Data.ProjectOf(cv.ID); pr != nil {
		project = pr.Name
	}
	anchor.Tooltip(sidebar.Title(c, cv) + "\n" + project)
}
