package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// projectControls marks what the transcript's message actions can do this
// frame, and the message edited in its bubble. An edit whose turn left the
// history ends.
func (r *Runtime) projectControls(c *ui.Context, cv *viewmodel.Conversation, comp *composer.State) {
	x := &conversationActions{r: r, c: c, cv: cv, comp: comp}
	session, ok := r.session(cv.ID)
	cv.Session.ReadOnly = !ok || delegated(session)
	edit, editing := r.conversationEdits[cv.ID]
	controls := x.pageControls()
	shown := false
	for i := range cv.Turns {
		turn := &cv.Turns[i]
		id, err := protocol.ParseTurnID(turn.ID)
		if err != nil {
			turn.Controls = conversation.Controls{ReadOnly: cv.Session.ReadOnly, NoEdit: true, NoFork: true, NoRewind: true, NoWorktree: true}
			continue
		}
		turn.Controls = controls(id)
		turn.Controls.NoExport = !r.controller.Connected()
		if editing && edit.Turn == id {
			shown = true
			turn.Editing = &conversation.Editing{Text: edit.Input.Text, Pending: edit.pending}
		}
	}
	// A published document exports while its Node answers.
	connected := r.controller.Connected()
	for i := range cv.Turns {
		for _, block := range cv.Turns[i].Reply.Blocks {
			for f := range block.Files {
				block.Files[f].Exportable = block.Files[f].Document && connected
			}
		}
	}
	if editing && !shown && !edit.pending {
		delete(r.conversationEdits, cv.ID)
	}
}
