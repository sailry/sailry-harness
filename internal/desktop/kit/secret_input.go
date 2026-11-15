package kit

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
)

// SecretInputProps are a SecretInput's options.
type SecretInputProps struct {
	Label       string
	Placeholder string
	Disabled    bool
}

// SecretInput edits a credential, masked unless *shown is true, and returns
// the MyGo input so callers can observe edits, submissions and focus.
// MyGo has no trailing input control slot; CrychicUI's PasswordField adds a
// strength meter and a hardcoded reveal label. The inset Kit button keeps
// native editing and masking, with a localized action inside the input.
func SecretInput(c *ui.Context, value *string, shown *bool, o SecretInputProps) ui.Element {
	var input ui.Element
	ui.Box(c).FillWidth().Children(func() {
		input = ui.TextInput(c.Key("input"), value).Label(o.Label).Placeholder(o.Placeholder).
			Disabled(o.Disabled).PaddingEnd(32).FillWidth()
		icon, label := icons.Eye, "mcp_show_secret"
		if *shown {
			icon, label = icons.EyeOff, "mcp_hide_secret"
		} else {
			input.Password()
		}
		IconButton(c.Key("reveal"), icon, i18n.T(c, label), 24, false).
			Disabled(o.Disabled).KeepFocus().Controls(input).
			Absolute().Top(0).Bottom(0).End(4).MarginY(ui.Auto).
			OnClick(func() { *shown = !*shown })
	})
	return input
}
