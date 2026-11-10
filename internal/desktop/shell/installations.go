package shell

import "github.com/sailry/sailry-harness/internal/desktop/transfers"

func (a *App) SetInstallations(queue *transfers.Queue) { a.installs = queue }
