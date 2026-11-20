package store

import (
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

var (
	queueRevisionChanged        = protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "queue revision changed; reload before editing"}
	queueItemNotPending         = protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "turn is no longer queued"}
	queueInputConsumed          = protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "turn input has already been consumed"}
	queueInputNotEditable       = protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "turn input is no longer editable"}
	guidanceNotPending          = protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "queued input has no pending guidance"}
	guidanceRecipientStopping   = protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "guidance recipient is stopping"}
	guidanceRecipientNotRunning = protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "guidance recipient is no longer running"}
	guidanceRecipientNotTask    = protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "guidance recipient is not a task"}
	guidanceRecipientIsSource   = protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "queued guidance cannot address itself"}
	guidanceScopeMismatch       = protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "guidance belongs to another execution scope"}
)

var queueRejections = map[protocol.Fault]struct {
	reason   string
	race     bool
	commands []string
}{
	queueRevisionChanged:        {"queue_revision_changed", true, []string{"edit_queued_turn", "remove_queued_turn", "send_queued_turn", "guide_queued_turn", "withdraw_guidance", "move_queued_turn", "set_queue_paused"}},
	queueItemNotPending:         {"queue_item_not_pending", true, []string{"edit_queued_turn", "remove_queued_turn", "send_queued_turn", "guide_queued_turn", "withdraw_guidance", "move_queued_turn"}},
	queueInputConsumed:          {"queue_input_consumed", true, []string{"send_queued_turn", "guide_queued_turn", "start_queued_turn"}},
	queueInputNotEditable:       {"queue_input_not_editable", true, []string{"edit_queued_turn"}},
	guidanceNotPending:          {"guidance_not_pending", true, []string{"withdraw_guidance"}},
	guidanceRecipientStopping:   {"recipient_stopping", true, []string{"guide_queued_turn"}},
	guidanceRecipientNotRunning: {"recipient_not_running", true, []string{"guide_queued_turn"}},
	guidanceRecipientNotTask:    {"recipient_not_task", false, []string{"guide_queued_turn"}},
	guidanceRecipientIsSource:   {"recipient_is_source", false, []string{"guide_queued_turn"}},
	guidanceScopeMismatch:       {"guidance_scope_mismatch", false, []string{"guide_queued_turn"}},
}

// QueueRejection identifies the store's exact refusals after a command result
// has crossed the durable request ledger. The bool distinguishes a queue race
// from an invalid recipient or execution scope. Unknown faults stay unclassified.
func QueueRejection(kind string, failure *protocol.Fault) (reason string, race bool) {
	if failure == nil || failure.Details != nil {
		return "", false
	}
	key := protocol.Fault{Code: failure.Code, Message: failure.Message}
	if refusal, ok := queueRejections[key]; ok && slices.Contains(refusal.commands, kind) {
		return refusal.reason, refusal.race
	}
	return "", false
}
