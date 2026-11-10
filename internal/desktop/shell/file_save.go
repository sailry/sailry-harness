package shell

import "github.com/egoist/mygo/ui"

func (a *App) updateFileSaveMenu(c *ui.Context) {
	if a.Preview {
		return
	}
	enabled := false
	if !a.dialog() {
		if project := a.project(); project != nil {
			enabled = project.CanSaveFocused(c)
		}
	}
	if enabled == a.fileSaveEnabled {
		return
	}
	a.fileSaveEnabled = enabled
	if a.fileSaveMenu != nil {
		a.fileSaveMenu(enabled)
	}
}
