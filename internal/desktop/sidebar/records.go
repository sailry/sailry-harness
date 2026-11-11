package sidebar

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

type recordsState struct {
	open, pending bool
	opening       string // the record being opened
	project       string
	choose        func(*viewmodel.Conversation, func(bool))
}

func (s *State) ShowRecords(project string, choose func(*viewmodel.Conversation, func(bool))) {
	s.records = recordsState{open: true, project: project, choose: choose}
}
func (s *State) recordsDialog(c *ui.Context) {
	records := &s.records
	if !records.open {
		return
	}
	palette := theme.Of(c)
	width, _ := c.Size()
	title := i18n.T(c, "workspace_records")
	kit.Dialog(c.Key("project-records"), &records.open, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600).TextColor(palette.PopoverForeground)
		ui.Scroll(c.Key("records")).MaxHeight(360).Children(func() {
			for _, project := range s.Data.Projects {
				if project.ID != records.project {
					continue
				}
				for _, cv := range project.Conversations {
					icon := icons.Bot
					if cv.Archived {
						icon = icons.Inbox
					}
					kit.Ghost(c.Key(cv.ID), Title(c, cv), false).FillWidth().Height(40).PaddingX(12).Gap(8).Disabled(records.pending).Children(func() {
						ui.Icon(c, icon).FontSize(16).TextColor(palette.MutedForeground)
						ui.Text(c, Title(c, cv)).FontSize(14).Grow(1).Shrink(1).SingleLine()
						if records.pending && records.opening == cv.ID {
							kit.Busy(c)
						} else {
							ui.Icon(c, icons.ChevronRight).FontSize(14).TextColor(palette.MutedForeground)
						}
					}).OnClick(func() {
						if records.choose == nil {
							return
						}
						records.pending, records.opening = true, cv.ID
						records.choose(cv, func(ok bool) {
							records.pending = false
							if ok {
								records.open = false
							}
						})
					})
				}
			}
		})
	}).Width(min(560, width-48)).Label(title)
}
