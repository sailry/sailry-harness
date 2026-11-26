package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	view "github.com/sailry/sailry-harness/internal/desktop/viewapi"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (h *windowViewHost) mountTerminal(id protocol.TerminalID, allows func(protocol.TerminalInfo) bool) view.Terminal {
	r := h.runtime
	if id == (protocol.TerminalID{}) || allows == nil || !h.open() || r.App.DemoOn() {
		return view.Terminal{}
	}
	var mounted *pluginNativeTerminal
	var seen, connected, closed bool
	var focus string
	release := func() {
		if mounted != nil {
			mounted.release()
			mounted = nil
		}
	}
	return view.Terminal{
		Render: func(c *ui.Context) {
			if closed || !h.open() || r.App.DemoOn() {
				return
			}
			seen = true
			now := r.controller.Connected()
			if mounted != nil && now != connected {
				if now {
					release()
				} else {
					mounted.cancel()
				}
			}
			connected = now
			if mounted == nil && connected {
				mounted = r.mountEmbeddedTerminal(r.ctx, id, r.controller.Do, allows)
			}
			if mounted == nil || mounted.state.failed {
				kit.Result(c, kit.ResultProps{Icon: icons.Terminal, Title: i18n.T(c, "unavailable")}).Grow(1).Justify(ui.Center)
				return
			}
			mounted.draw(c.Key(id), r.terminalDisplay(), !connected, &focus)
		},
		Focus: func() {
			if !closed && h.open() && !r.App.DemoOn() {
				focus = id.String()
				r.wake()
			}
		},
		Close: h.trackMount(nativeWindowResource{
			afterFrame: func() {
				if !seen || r.App.DemoOn() {
					release()
				}
				seen = false
			},
			close: func() {
				closed = true
				release()
			},
		}),
	}
}
