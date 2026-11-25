package panel

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

type PickerRow struct {
	Label, Detail, Group string
	Icon                 *ui.SVG
	Checked, Disabled    bool
	Run                  func(*ui.Context)
}

func WorkspacePicker(title string, rows []PickerRow) func(*ui.Context) bool {
	open, selected, query := true, 0, ""
	items := make([]kit.CommandItem, 0, len(rows))
	actions := make([]func(*ui.Context), 0, len(rows))
	var previous string
	initialized := false
	return func(c *ui.Context) bool {
		if !open {
			return false
		}
		filter := strings.ToLower(strings.TrimSpace(query))
		if !initialized || previous != filter {
			items, actions = items[:0], actions[:0]
			for _, row := range rows {
				if strings.Contains(strings.ToLower(row.Label), filter) {
					items = append(items, kit.CommandItem{Label: row.Label, Detail: row.Detail, Group: row.Group, Icon: row.Icon,
						Checked: row.Checked, Disabled: row.Disabled || row.Run == nil})
					actions = append(actions, row.Run)
				}
			}
			previous, initialized = filter, true
		}
		chosen := false
		kit.Command(c, "workspace-picker", &open, &query, &selected, items, kit.CommandProps{
			Placeholder: title, Empty: i18n.T(c, "git_no_matches"),
		}, func(index int) {
			open, chosen = false, true
			actions[index](c)
		})
		// A row can replace this overlay with the next picker.
		return open || chosen
	}
}
