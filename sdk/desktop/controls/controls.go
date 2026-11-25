// Package controls shares the Desktop's themed settings controls with native pages.
package controls

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

type SelectProps = kit.SelectProps
type SelectOption = kit.SelectOption
type ResultProps = kit.ResultProps

func Group(c *ui.Context, key any, title, description string, actions, body func()) {
	kit.Group(c, key, title, description, actions, body)
}

func Card(c *ui.Context, rows func()) ui.Element { return kit.Card(c, rows) }

func SwitchRow(c *ui.Context, key any, title, description string, value *bool) bool {
	return kit.SwitchRow(c, key, title, description, value)
}

func WideRow(c *ui.Context, key any, title, description string, control func()) ui.Element {
	return kit.WideRow(c, key, title, description, control)
}

func SelectField(c *ui.Context, id string, chosen *string, props SelectProps) (ui.Element, bool) {
	return kit.SelectField(c, id, chosen, props)
}

func PrimaryBusy(c *ui.Context, label string, disabled, busy bool) ui.Element {
	return kit.PrimaryBusy(c, label, disabled, busy)
}

func Result(c *ui.Context, props ResultProps) ui.Element { return kit.Result(c, props) }

func Entry(c *ui.Context, key any, content, control, actions func()) ui.Element {
	return kit.Entry(c, key, content, control, actions)
}

func Record(c *ui.Context, title, detail string) { kit.Record(c, title, detail) }

func Action(c *ui.Context, icon *ui.SVG, label string, disabled bool) ui.Element {
	return kit.Action(c, icon, label, disabled)
}
