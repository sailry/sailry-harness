package conversation

import (
	"fmt"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/theme"
)

// The session around the turns: the first page loading, older pages
// loading as the start comes into view (paging.rs), the connection's
// banner while it recovers (recovery.rs), a diagnostic of a failure
// (diagnostic.rs), and the selection's Quote and Copy (selection.rs).

// loading is the transcript before its first page: a loader and its label.
func loading(c *ui.Context) {
	p := theme.Of(c)
	ui.Column(c.Key("loading")).Grow(1).Center().Gap(12).Children(func() {
		kit.Loader(c, 20)
		ui.Text(c, i18n.T(c, "chat_history_loading")).FontSize(14).TextColor(p.MutedForeground)
	})
}

// older is the row before the first turn while there is history before
// it: a loader while the page loads, its failure with Retry, or nothing,
// as reaching it asks for the page (paging.rs).
func (s *State) older(c *ui.Context, sess *Session, actions Actions) {
	if sess == nil || sess.Older == 0 {
		return
	}
	p := theme.Of(c)
	ui.Row(c.Key("older")).Height(32).Gap(8).Justify(ui.Center).AlignItems(ui.Center).TextColor(p.MutedForeground).Children(func() {
		switch {
		case sess.OlderFailed:
			ui.Text(c, i18n.T(c, "chat_history_failed")).FontSize(14).TextColor(p.Danger)
			kit.Ghost(c.Key("retry"), i18n.T(c, "chat_retry"), false).Height(28).PaddingX(8).
				Children(func() { ui.Text(c, i18n.T(c, "chat_retry")).FontSize(14) }).OnClick(actions.LoadOlder)
		default:
			kit.Loader(c, 14)
			if !sess.LoadingOlder {
				// The start came into view: ask once this frame is built.
				s.wantOlder = true
			}
		}
	})
}

// banner tells the connection is recovering or lost, with Retry.
func banner(c *ui.Context, sess *Session, actions Actions) {
	if sess == nil || sess.Connection == Connected {
		return
	}
	p := theme.Of(c)
	ui.Row(c.Key("banner")).FillWidth().MaxWidth(theme.ContentWidth).Margin(8, ui.Auto, 0, ui.Auto).Padding(6, 8, 6, 12).Gap(8).
		Radius(p.RadiusLg).Background(p.Floating()).Border(1, p.Border).AlignItems(ui.Center).Role(ui.RoleStatus).Children(func() {
		if sess.Connection == Reconnecting {
			kit.Loader(c, 14)
			kit.Shimmer(c, i18n.T(c, "chat_reconnecting"), 14, p.MutedForeground).Grow(1)
		} else {
			ui.Icon(c, icons.TriangleAlert).FontSize(16).TextColor(p.Danger)
			ui.Text(c, i18n.T(c, "plugins_view_offline")).FontSize(14).Grow(1).TextColor(p.Danger)
		}
		kit.Ghost(c.Key("reconnect"), i18n.T(c, "chat_retry"), false).Height(28).PaddingX(8).
			Children(func() { ui.Text(c, i18n.T(c, "chat_retry")).FontSize(14) }).OnClick(actions.Reconnect)
	})
}

// diagnostic is a failure in detail after the last turn: its heading,
// each field as a label and value, and the raw message, with Copy.
func diagnostic(c *ui.Context, d *Diagnostic) {
	if d == nil {
		return
	}
	p := theme.Of(c)
	ui.Column(c.Key("diagnostic")).FillWidth().Padding(12).Gap(8).Radius(p.RadiusLg).Border(1, p.Danger.Alpha(0.4)).
		Background(p.Danger.Alpha(0.06)).Children(func() {
		ui.Row(c).Gap(8).AlignItems(ui.Center).Children(func() {
			ui.Icon(c, icons.TriangleAlert).FontSize(16).TextColor(p.Danger)
			ui.Text(c, d.Title).FontSize(14).FontWeight(500).Grow(1).TextColor(p.Danger)
			kit.IconButton(c.Key("copy"), icons.Copy, i18n.T(c, "content_copy"), 28, false).OnClick(func() {
				c.WriteClipboard(DiagnosticText(d))
				c.Toast(i18n.T(c, "content_copied"))
			})
		})
		for i, f := range d.Fields {
			ui.Row(c.Key(i)).Gap(12).Children(func() {
				ui.Text(c, f[0]).FontSize(13).Width(120).Shrink(0).TextColor(p.MutedForeground)
				ui.Text(c, f[1]).Selectable().Font("monospace").FontSize(13).Grow(1).Shrink(1).MinWidth(0)
			})
		}
		if d.Detail != "" {
			ui.Scroll(c).MaxHeight(120).Children(func() {
				ui.Text(c, d.Detail).Selectable().Font("monospace").FontSize(12).TextColor(p.MutedForeground)
			})
		}
	})
}

// DiagnosticText is what Copy copies of a diagnostic: the heading, the
// fields as "label: value" lines, and the message.
func DiagnosticText(d *Diagnostic) string {
	var b strings.Builder
	b.WriteString(d.Title)
	for _, f := range d.Fields {
		fmt.Fprintf(&b, "\n%s: %s", f[0], f[1])
	}
	if d.Detail != "" {
		b.WriteString("\n\n" + d.Detail)
	}
	return b.String()
}

// Quote is a selection as the composer quotes it: each line after "> ",
// then a blank line (selection.rs insert_quote).
func Quote(text string) string {
	lines := strings.Split(strings.TrimRight(text, "\n"), "\n")
	for i, l := range lines {
		lines[i] = "> " + l
	}
	return strings.Join(lines, "\n") + "\n\n"
}

// selection is the pill over a turn holding a selection: Quote and Copy
// (selection.rs selection_toolbar), at the turn's top end.
func selection(c *ui.Context, turn ui.Element, actions Actions) {
	sel := turn.SelectedTexts()
	if len(sel) == 0 {
		return
	}
	var parts []string
	for _, t := range sel {
		r := []rune(t.Text)
		parts = append(parts, string(r[min(t.Start, len(r)):min(t.End, len(r))]))
	}
	text := strings.Join(parts, "\n")
	if strings.TrimSpace(text) == "" {
		return
	}
	kit.Floating(c, ui.Row(c.Key("selection"))).Absolute().Top(-4).Right(padX).Padding(4).Gap(4).Radius(18).Children(func() {
		pill := func(key, label string, do func()) {
			kit.Ghost(c.Key(key), label, false).Height(28).PaddingX(10).Radius(14).
				Children(func() { ui.Text(c, label).FontSize(14) }).OnClick(do)
		}
		pill("quote", i18n.T(c, "message_quote"), func() { actions.Quote(Quote(text)) })
		pill("copy", i18n.T(c, "message_copy"), func() {
			c.WriteClipboard(text)
			c.Toast(i18n.T(c, "content_copied"))
		})
	})
}
