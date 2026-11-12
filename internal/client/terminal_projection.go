package client

import (
	"math"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// The bounds a Node keeps a terminal screen within.
const (
	terminalMaxColumns    = 500
	terminalMaxRows       = 200
	terminalMaxScrollback = 1000
	terminalMaxGraphics   = 8 * 1024 * 1024
)

// terminalProjection reduces one terminal's updates to its latest snapshot.
//
// A Node numbers the frames of each subscription from its own snapshot, so
// order is compared within one subscription only: the first snapshot after
// reconnect replaces what an earlier subscription left.
type terminalProjection struct {
	node     protocol.NodeID
	terminal protocol.TerminalID
	snapshot *protocol.TerminalSnapshot
	fresh    bool
	recovery bool
}

func newTerminalProjection(node protocol.NodeID, terminal protocol.TerminalID) *terminalProjection {
	return &terminalProjection{node: node, terminal: terminal, fresh: true, recovery: true}
}

// reconnect starts a new subscription: frames wait for its snapshot.
func (projection *terminalProjection) reconnect() {
	projection.fresh, projection.recovery = true, true
}

func (projection *terminalProjection) recover() (Apply, error) {
	projection.recovery = true
	return Recover, nil
}

func (projection *terminalProjection) apply(update protocol.Update) (Apply, error) {
	switch update.Kind {
	case "terminal_snapshot":
		snapshot, err := payload[protocol.TerminalSnapshot](update.Data)
		if err != nil {
			return Ignored, err
		}
		if err := projection.target(snapshot.Node, snapshot.Info.ID); err != nil {
			return Ignored, err
		}
		if current := projection.snapshot; current != nil && !projection.fresh &&
			(snapshot.Info.Revision < current.Info.Revision ||
				snapshot.Info.Revision == current.Info.Revision && snapshot.Sequence < current.Sequence) {
			return Ignored, nil
		}
		if !validTerminalScreen(snapshot.Screen) {
			return projection.recover()
		}
		projection.snapshot, projection.fresh, projection.recovery = &snapshot, false, false
		return Applied, nil
	case "terminal_frame":
		frame, err := payload[protocol.TerminalFrame](update.Data)
		if err != nil {
			return Ignored, err
		}
		if err := projection.target(frame.Node, frame.Info.ID); err != nil {
			return Ignored, err
		}
		current := projection.snapshot
		if current == nil || projection.fresh {
			return projection.recover()
		}
		if frame.Sequence <= current.Sequence {
			return Ignored, nil
		}
		if projection.recovery || frame.Sequence != current.Sequence+1 {
			return projection.recover()
		}
		var screen protocol.TerminalScreen
		switch frame.Screen.Kind {
		case "replace":
			replacement, err := payload[protocol.TerminalScreenScreenUpdateReplacePayload](frame.Screen.Data)
			if err != nil {
				return projection.recover()
			}
			screen = replacement.Screen
		case "patch":
			change, err := payload[protocol.TerminalScreenScreenUpdatePatchPayload](frame.Screen.Data)
			dropped := uint64(change.Patch.DroppedScrollbackRows)
			if err != nil || dropped > uint64(len(current.Screen.Scrollback)) {
				return projection.recover()
			}
			patch := change.Patch
			scrollback := make([]protocol.TerminalScreenLine, 0, len(current.Screen.Scrollback)-int(dropped)+len(patch.AppendedScrollback))
			scrollback = append(append(scrollback, current.Screen.Scrollback[dropped:]...), patch.AppendedScrollback...)
			screen = protocol.TerminalScreen{
				Columns: patch.Columns, Scrollback: scrollback, Rows: patch.Rows, Cursor: patch.Cursor,
				Foreground: patch.Foreground, Background: patch.Background, CursorColor: patch.CursorColor,
				BracketedPaste: patch.BracketedPaste, MouseTracking: patch.MouseTracking, Alternate: patch.Alternate,
				Features: patch.Features, Graphics: current.Screen.Graphics,
			}
			if patch.Graphics != nil {
				screen.Graphics = *patch.Graphics
			}
		default:
			// A screen change this build does not declare cannot be reduced.
			return projection.recover()
		}
		if !validTerminalScreen(screen) {
			return projection.recover()
		}
		projection.snapshot = &protocol.TerminalSnapshot{Node: frame.Node, Info: frame.Info, Sequence: frame.Sequence, Screen: screen}
		return Applied, nil
	case "reset_required":
		return projection.recover()
	}
	return Ignored, fault(protocol.ErrorCodeInvalidRequest, "unexpected terminal subscription update")
}

func (projection *terminalProjection) target(node protocol.NodeID, terminal protocol.TerminalID) error {
	if node != projection.node || terminal != projection.terminal {
		return fault(protocol.ErrorCodeWrongTarget, "terminal update belongs to another resource")
	}
	return nil
}

// validTerminalScreen checks a peer's screen before it is shown. A screen
// that carries its content as a VT stream names the viewport there and has no
// parsed rows; one without has at least one row.
func validTerminalScreen(screen protocol.TerminalScreen) bool {
	if screen.Columns < 1 || screen.Columns > terminalMaxColumns ||
		len(screen.Rows) > terminalMaxRows || len(screen.Scrollback) > terminalMaxScrollback {
		return false
	}
	if screen.VT == nil {
		if len(screen.Rows) == 0 {
			return false
		}
	} else if viewport := screen.VT.Viewport; viewport.Columns < 1 || viewport.Columns > terminalMaxColumns ||
		viewport.Rows < 1 || viewport.Rows > terminalMaxRows {
		return false
	}
	if cursor := screen.Cursor; cursor != nil && (cursor.Column >= screen.Columns || int(cursor.Row) >= len(screen.Rows)) {
		return false
	}
	images := make(map[uint32]protocol.TerminalGraphicsImage, len(screen.Graphics.Images))
	var allocation uint64
	for _, image := range screen.Graphics.Images {
		allocation += uint64(image.Width) * uint64(image.Height) * 4
		if _, repeated := images[image.ID]; repeated || image.Width == 0 || image.Height == 0 ||
			allocation > terminalMaxGraphics || len(image.Png) > terminalMaxGraphics*2 {
			return false
		}
		images[image.ID] = image
	}
	for _, placement := range screen.Graphics.Placements {
		image, placed := images[placement.Image]
		x, y, width, height := placement.Source[0], placement.Source[1], placement.Source[2], placement.Source[3]
		if !placed || width > math.MaxUint32-x || x+width > image.Width || height > math.MaxUint32-y || y+height > image.Height ||
			placement.Cell[0] == 0 || placement.Cell[1] == 0 ||
			placement.Tile != nil && (placement.Tile[0] >= placement.Tile[2] || placement.Tile[1] >= placement.Tile[3]) {
			return false
		}
	}
	for _, lines := range [][]protocol.TerminalScreenLine{screen.Rows, screen.Scrollback} {
		for _, line := range lines {
			end := 0
			for _, span := range line.Spans {
				if int(span.Column) < end || span.Columns == 0 {
					return false
				}
				if end = int(span.Column) + int(span.Columns); end > int(screen.Columns) {
					return false
				}
			}
		}
	}
	return true
}
