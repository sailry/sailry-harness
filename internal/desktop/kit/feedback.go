package kit

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Message preserves the context toast's message identity and default look while
// applying this window's selected duration.
func Message(c *ui.Context, message string) {
	c.AddToast(ui.Toast{ID: message, Title: message, Timeout: ToastDuration(c)})
}

// Error is an error toast with an optional title over the problem's detail,
// which stays until closed so it is read. Views keep their content and
// drafts; the toast is the only place a failure shows.
func Error(c *ui.Context, title, detail string) {
	c.AddToast(ui.Toast{Type: "error", Title: title, Description: detail, Timeout: -1})
}

// Notify is a neutral toast with an optional title over its detail.
func Notify(c *ui.Context, title, detail string) {
	if title == "" {
		title, detail = detail, ""
	}
	c.AddToast(ui.Toast{Type: "info", Title: title, Description: detail, Timeout: ToastDuration(c)})
}

// Retry is an error toast whose action retries what failed: the action closes
// the toast and runs again.
func Retry(c *ui.Context, message string, again func()) {
	c.AddToast(ui.Toast{Type: "error", Title: message, Timeout: -1, Action: i18n.T(c, "chat_retry"), OnAction: again})
}

// Details is a dialog up to 560 wide and 80% of the window high with the
// title, a muted subtitle ("" for none) and the detail as selectable
// Markdown. It shows while *open.
func Details(c *ui.Context, open *bool, title, subtitle, detail string) {
	if !*open {
		return
	}
	p := theme.Of(c)
	w, h := c.Size()
	Dialog(c.Key("details"), open, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600).TextColor(p.PopoverForeground).PaddingEnd(24)
		ui.Scroll(c.Key("detail")).Grow(1).Shrink(1).Gap(12).Children(func() {
			if subtitle != "" {
				ui.Text(c, subtitle).FontSize(14).TextColor(p.MutedForeground)
			}
			Markdown(c, detail, MarkdownProps{})
		})
	}).Width(min(560, max(w-48, 0))).MaxHeight(h * 0.8).Label(title)
}
