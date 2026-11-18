package live

import (
	"encoding/hex"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) narrowConnection(device viewmodel.Device, selected string) {
	state := r.connections
	direction := link.PeerDirection(selected)
	raw, err := hex.DecodeString(device.ID)
	if err != nil || len(raw) != len(protocol.NodeID{}) || state == nil || state.endpoint == nil || device.Local || device.Direction != "both" || direction != link.DirectionHost && direction != link.DirectionController || state.narrowing[device.ID] {
		return
	}
	id := protocol.NodeID(raw)
	if state.narrowing == nil {
		state.narrowing = map[string]bool{}
	}
	state.narrowing[device.ID] = true
	r.async(func() {
		err := state.endpoint.SetDirection(r.ctx, id, direction)
		r.enqueue(func(c *ui.Context) {
			delete(state.narrowing, device.ID)
			if err != nil {
				r.reportConnection(c, err, "pairing_failed")
				return
			}
			kit.Toast(c, kit.Done, i18n.T(c, "connections_direction_saved"))
		})
	})
}
