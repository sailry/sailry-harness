package live

import (
	"time"

	"github.com/egoist/mygo/ui"
)

// Watch invalidations follow the source component's 750 ms quiet-refresh floor;
// explicit refresh remains immediate. One timer coalesces intervening events.
func (p *workspaceProject) refreshWatchedGit() {
	remaining := 750*time.Millisecond - time.Since(p.gitLastRead)
	if remaining > 0 {
		if p.gitRefreshTimer == nil {
			generation := p.gitRefreshGeneration
			p.gitRefreshTimer = time.AfterFunc(remaining, func() {
				p.runtime.enqueue(func(*ui.Context) {
					if generation != p.gitRefreshGeneration {
						return
					}
					p.gitRefreshTimer = nil
					p.pendingGit = true
				})
			})
		}
		return
	}
	p.stopGitRefreshTimer()
	p.pendingGit = false
	p.readGit()
}
func (p *workspaceProject) stopGitRefreshTimer() {
	p.gitRefreshGeneration++
	if p.gitRefreshTimer != nil {
		p.gitRefreshTimer.Stop()
		p.gitRefreshTimer = nil
	}
}
