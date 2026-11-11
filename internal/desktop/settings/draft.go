package settings

import (
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// field is a draft dialog's input: its label's and placeholder's i18n
// keys and the value it edits.
type field struct {
	label, hint string
	value       *string
	secret      bool
}

// draftDialog is a dialog titled by title holding a column of labeled inputs,
// with Cancel and Save. save returns the i18n key of what is wrong, "" when
// it saved; a problem shows as a toast and the dialog stays open with the
// draft.
func draftDialog(c *ui.Context, key any, open *bool, title string, fields []field, save func() string) {
	if !*open {
		return
	}
	p := theme.Of(c)
	t := i18n.T(c, title)
	kit.Dialog(c.Key(key), open, func() {
		ui.Text(c, t).FontSize(16).FontWeight(600).TextColor(p.PopoverForeground)
		ui.Scroll(c.Key("body")).Grow(1).Shrink(1).Gap(16).Children(func() {
			for _, f := range fields {
				label := i18n.T(c, f.label)
				kit.Field(c, f.label, label, "", func() {
					in := ui.TextInput(c.Key("input"), f.value).Label(label).Placeholder(i18n.T(c, f.hint)).FillWidth()
					if f.secret {
						in.Password()
					}
				})
			}
		})
		footer(c, i18n.T(c, "settings_save"), func() { *open = false }, func() {
			if problem := save(); problem != "" {
				kit.Toast(c, kit.Failed, i18n.T(c, problem))
				return
			}
			*open = false
		})
	}).Width(min(560, width(c)-48)).Label(t)
}

// footer is a dialog's footer: Cancel and the primary action, at the end.
func footer(c *ui.Context, action string, cancel, ok func(), pending ...bool) {
	ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Children(func() {
		kit.Secondary(c.Key("cancel"), i18n.T(c, "settings_cancel")).OnClick(cancel)
		kit.PrimaryBusy(c.Key("ok"), action, false, slices.Contains(pending, true)).OnClick(ok)
	})
}

// width is the window's width.
func width(c *ui.Context) float32 {
	w, _ := c.Size()
	return w
}
