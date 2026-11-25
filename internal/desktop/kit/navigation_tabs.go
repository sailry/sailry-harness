package kit

import "github.com/egoist/mygo/ui"

type NavigationTab struct {
	ID, Label, CloseLabel            string
	Icon                             *ui.SVG
	Closable, CloseDisabled, Loading bool
}

type NavigationTabsProps struct {
	Selected, CloseLabel string
	MaxWidth             float32
	Disabled             bool
	Select, Close        func(string)
}

func NavigationTabs(c *ui.Context, tabs []NavigationTab, props NavigationTabsProps) ui.Element {
	width := props.MaxWidth
	if width == 0 {
		width = 240
	}
	return ui.ScrollHorizontal(c).FillWidth().Children(func() {
		ui.Row(c).Gap(StripGap).AlignItems(ui.Center).Children(func() {
			for index, tab := range tabs {
				active := tab.ID == props.Selected
				if index > 0 {
					StripDivider(c, "divider-"+tab.ID)
				}
				button, hovered := StripTab(c, tab.ID, tab.Label, active, width)
				label := tab.CloseLabel
				if label == "" {
					label = props.CloseLabel
				}
				var close func()
				if tab.Closable && !tab.CloseDisabled && props.Close != nil {
					close = func() { props.Close(tab.ID) }
				}
				button = StripTabBody(c, button.Disabled(props.Disabled), tab.Icon, tab.Label, active, hovered, tab.Loading, label, close)
				if props.Select != nil {
					button.OnClick(func() { props.Select(tab.ID) })
				}
			}
		})
	})
}
