package live

import (
	"strings"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func archiveBusy(session protocol.Session, lane client.Lane) bool {
	if lane == client.LaneRunning || lane == client.LaneWaiting || session.Activity.Queued != 0 || session.Activity.Background != 0 {
		return true
	}
	if run := session.Activity.Run; run != nil {
		switch run.Status {
		case protocol.ConversationStatusQueued, protocol.ConversationStatusRunning, protocol.ConversationStatusStopping:
			return true
		}
	}
	return false
}

func (r *Runtime) canArchiveConversation(cv *viewmodel.Conversation) bool {
	if cv == nil || cv.Archived || cv.Offline {
		return false
	}
	owner := r.conversationRuntime(cv, nil)
	if owner == nil || !owner.controller.Connected() {
		return false
	}
	if strings.HasPrefix(cv.ID, "draft/") {
		return true
	}
	sessions := owner.snapshot().Sessions
	lanes := client.AggregateLanes(sessions)
	for i, session := range sessions {
		if session.ID.String() == cv.ID {
			return !archiveBusy(session, lanes[i])
		}
	}
	return false
}
