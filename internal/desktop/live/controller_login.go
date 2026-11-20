package live

import (
	"context"
	"errors"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// A recovery belongs to one waiting state, independent of the provider's
// ordinary sign-in selection. Its request and wait observation outlive the view.
type controllerLoginRecovery struct {
	executor    *Runtime
	input       protocol.CommandResumeControllerLoginPayload
	request     *protocol.Request
	releaseWait func()
	cancelReply context.CancelFunc
	waitEnded   bool
	busy        bool
	recheck     bool
	settled     bool
}

func canRestoreControllerLogin(node protocol.NodeID, waiting *protocol.ConversationRunWaiting) bool {
	return node != (protocol.NodeID{}) && waiting.Credential.Node == node &&
		waiting.Attempt != (protocol.RequestID{}) && waiting.Provider != (protocol.ProviderID{})
}

func (x *conversationActions) ResumeLogin(turn string) {
	_, snapshot, ok := x.state()
	if !ok || !x.r.controller.Connected() {
		kit.Toast(x.c, kit.Failed, i18n.T(x.c, "resource_offline_action"))
		return
	}
	for _, run := range snapshot.Page.Runs {
		if run.Turn.String() != turn || run.Status != protocol.ConversationStatusRunning || run.Waiting == nil || run.Waiting.Kind != "controller_login" {
			continue
		}
		waiting := run.Waiting
		if !canRestoreControllerLogin(x.r.controller.Target(), waiting) {
			break
		}
		root := x.r.localRuntime()
		credentialNode := waiting.Credential.Node
		key := credentialNode.String() + "/" + run.Turn.String() + "/" + waiting.Attempt.String()
		var recovery *controllerLoginRecovery
		if state := root.authorizations; state != nil {
			recovery = state.loginRecoveries[key]
		}
		if recovery != nil {
			if recovery.busy || recovery.settled {
				return
			}
			if recovery.request != nil {
				x.r.restoreControllerLogin(recovery)
				return
			}
		}
		owner := root.providerOwner(credentialNode.String())
		if owner == nil {
			kit.Toast(x.c, kit.Failed, i18n.T(x.c, "resource_offline_action"))
			return
		}
		owner.initProviders()
		root.initProviders()
		if _, exists := owner.loginProvider(waiting.Provider.String()); !exists {
			kit.Toast(x.c, kit.Failed, i18n.T(x.c, "controller_login_provider_missing"))
			return
		}
		if root.authorizations.loginRecoveries == nil {
			root.authorizations.loginRecoveries = map[string]*controllerLoginRecovery{}
		}
		if recovery == nil {
			recovery = &controllerLoginRecovery{input: protocol.CommandResumeControllerLoginPayload{
				Session: run.Session, Turn: run.Turn, Waiting: waiting.Attempt, Previous: waiting.Credential,
			}}
			root.authorizations.loginRecoveries[key] = recovery
		}
		if attempt := owner.authorizations.provider; attempt != nil && attempt.recovery == recovery && !attempt.connected && attempt.ctx.Err() == nil {
			owner.App.Settings.ShowProviderAuthorization(credentialNode.String(), attempt.provider, attempt.view)
			return
		}
		owner.beginProviderLoginFor(waiting.Provider.String(), recovery)
		return
	}
	kit.Toast(x.c, kit.Conflict, i18n.T(x.c, "controller_login_state_changed"))
}

func (r *Runtime) completeControllerLogin(c *ui.Context, attempt *liveAuthorization, replacement *protocol.CredentialRef) {
	recovery := attempt.recovery
	if replacement == nil || replacement.Node != r.controller.Target() || replacement.Node != recovery.input.Previous.Node {
		kit.Toast(c, kit.Failed, i18n.T(c, "controller_login_restore_failed"))
		return
	}
	if recovery.request != nil || recovery.busy || recovery.settled {
		return
	}
	owner := r.runtimeFor(replacement.Node.String())
	if owner == nil || owner.controller.Target() != replacement.Node || owner.ctx.Err() != nil {
		kit.Toast(c, kit.Failed, i18n.T(c, "controller_login_restore_failed"))
		return
	}
	recovery.input.Replacement = *replacement
	request := owner.controller.Prepare(protocol.Command{Kind: "resume_controller_login", Data: recovery.input})
	recovery.request = &request
	owner.runControllerLogin(recovery, true)
}

func (r *Runtime) restoreControllerLogin(recovery *controllerLoginRecovery) {
	r.runControllerLogin(recovery, false)
}

func (r *Runtime) runControllerLogin(recovery *controllerLoginRecovery, first bool) {
	request := recovery.request
	if request == nil || recovery.settled || request.Target != r.controller.Target() || r.ctx.Err() != nil || r.runtimeFor(request.Target.String()) != r {
		return
	}
	if recovery.executor != r {
		recovery.stopFollowingWait()
		recovery.executor = r
	}
	if recovery.busy || !first && !r.controller.Connected() {
		if !first {
			recovery.recheck = true
		}
		return
	}
	recovery.busy, recovery.recheck = true, false
	ctx := r.ctx
	if first {
		ctx, recovery.cancelReply = context.WithCancel(ctx)
	}
	if !r.localRuntime().async(func() {
		err, retain := r.controllerLoginResult(ctx, *request, first)
		r.enqueue(func(c *ui.Context) {
			recovery.stopWaitingForReply()
			recovery.busy = false
			if !retain {
				recovery.stopFollowingWait()
				recovery.request, recovery.executor = nil, nil
				recovery.settled, recovery.waitEnded, recovery.recheck = err == nil, false, false
			}
			reportControllerLogin(c, err)
		})
	}) {
		recovery.stopWaitingForReply()
		recovery.busy = false
		return
	}
	r.followControllerLoginWait(recovery, request)
}

func (r *Runtime) controllerLoginResult(ctx context.Context, request protocol.Request, first bool) (error, bool) {
	if first {
		_, err := r.controller.Execute(ctx, request)
		switch faultCode(err) {
		case protocol.ErrorCodeOutcomeUnknown:
			if ctx.Err() != nil {
				return err, true
			}
		case protocol.ErrorCodeUnavailable, protocol.ErrorCodeCancelled:
			return err, true
		default:
			return err, false
		}
	}
	resolution, err := r.controller.Resolve(ctx, request)
	if err != nil {
		return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "the login recovery outcome could not be inspected"}, true
	}
	_, err = resolution.Result()
	// Unlike a failed inspection, a recorded rejection or confirmed absence
	// allows a new attempt if the same wait still offers sign-in.
	switch resolution.State {
	case client.RequestApplied, client.RequestFailed, client.RequestAbsent:
		return err, false
	default:
		return err, true
	}
}

func (recovery *controllerLoginRecovery) stopWaitingForReply() {
	if recovery.cancelReply != nil {
		recovery.cancelReply()
		recovery.cancelReply = nil
	}
}

func (recovery *controllerLoginRecovery) stopFollowingWait() {
	if recovery.releaseWait != nil {
		recovery.releaseWait()
		recovery.releaseWait = nil
	}
}

func (r *Runtime) followControllerLoginWait(recovery *controllerLoginRecovery, request *protocol.Request) {
	if recovery.releaseWait != nil || recovery.waitEnded {
		return
	}
	input := recovery.input
	observe := func(*ui.Context) {
		if recovery.executor != r || recovery.request != request || recovery.waitEnded || r.ctx.Err() != nil {
			return
		}
		state, ok := r.controller.Conversation(input.Session)
		if !ok || !state.Removed && (!state.Connected || state.Snapshot.Node != request.Target || state.Snapshot.Page.Session != input.Session) {
			return
		}
		for _, run := range state.Snapshot.Page.Runs {
			if run.Turn == input.Turn && run.Status == protocol.ConversationStatusRunning && run.Waiting != nil &&
				run.Waiting.Kind == "controller_login" && run.Waiting.Attempt == input.Waiting && run.Waiting.Credential == input.Previous {
				return
			}
		}
		// A cancelled or changed wait also disappears. Only the ledger can
		// confirm what this recovery did. Stop waiting for the reply without
		// stopping the admitted write.
		recovery.waitEnded = true
		recovery.stopFollowingWait()
		recovery.stopWaitingForReply()
		r.restoreControllerLogin(recovery)
	}
	release, err := r.controller.WatchConversation(r.ctx, input.Session, func() { r.enqueue(observe) })
	if err == nil {
		recovery.releaseWait = release
		observe(nil)
	}
}

func (r *Runtime) retryControllerLogins(all bool) {
	state := r.localRuntime().authorizations
	if state == nil || len(state.loginRecoveries) == 0 {
		return
	}
	node := r.controller.Target()
	for _, recovery := range state.loginRecoveries {
		if recovery.request != nil && recovery.request.Target == node &&
			(all || recovery.executor != r || recovery.recheck && !recovery.busy) {
			r.restoreControllerLogin(recovery)
		}
	}
}

func reportControllerLogin(c *ui.Context, err error) {
	if err == nil {
		kit.Toast(c, kit.Done, i18n.T(c, "controller_login_restored"))
		return
	}
	key, outcome := "controller_login_restore_failed", kit.Failed
	switch faultCode(err) {
	case protocol.ErrorCodeOutcomeUnknown:
		key, outcome = "controller_login_restore_unknown", kit.Uncertain
	case protocol.ErrorCodeConflict, protocol.ErrorCodeRevisionConflict:
		key, outcome = "controller_login_state_changed", kit.Conflict
	}
	var fault protocol.Fault
	var pointer *protocol.Fault
	if errors.As(err, &pointer) {
		fault = *pointer
	} else {
		errors.As(err, &fault)
	}
	if fault.Details != nil && fault.Details.Kind == "controller_login" {
		if details, ok := detail[protocol.FaultDetailsControllerLoginPayload](fault.Details.Data); ok {
			switch details.Reason {
			case "account_mismatch":
				key = "controller_login_account_mismatch"
			case "identity_unavailable":
				key = "controller_login_identity_unavailable"
			case "target_changed":
				key = "controller_login_target_changed"
			}
		}
	}
	kit.Toast(c, outcome, i18n.T(c, key))
	kit.RecordFailure("desktop", "controller_login_restore_failed", outcome, err)
}
