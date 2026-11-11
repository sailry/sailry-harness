package panel

import "github.com/egoist/mygo/ui"

// PickerRow is a row of a controller's picker: a Choice with a heading, an
// icon, the current one's check and a row that is shown but cannot be chosen.
type PickerRow struct {
	Label, Detail, Group string
	Icon                 *ui.SVG
	Checked, Disabled    bool
	Run                  func(*ui.Context)
}

// Pick opens the Git picker with rows, as Choose does with plain choices.
func (pr *Project) Pick(title string, rows []PickerRow) {
	items := make([]action, 0, len(rows))
	for _, row := range rows {
		items = append(items, action{label: row.Label, detail: row.Detail, group: row.Group, icon: row.Icon,
			checked: row.Checked, disabled: row.Disabled || row.Run == nil, run: row.Run})
	}
	pr.Git.picker = picker{open: true, title: title, items: items}
}
