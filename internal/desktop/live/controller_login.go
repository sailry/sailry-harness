package live

import (
	"errors"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// A recovery belongs to one waiting state, independent of the provider's
// ordinary sign-in selection. Its request survives an unobserved response.
type controllerLoginRecovery struct {
	executor *Runtime
	input    protocol.CommandResumeControllerLoginPayload
	request  *protocol.Request
	busy     bool
	settled  bool
}

func (x *conversationActions) ResumeLogin(turn string) {
	_, snapshot, ok := x.state()
	if !ok || !x.r.controller.Connected() {
		kit.Toast(x.c, kit.Failed, i18n.T(x.c, "resource_offline_action"))
		return
	}
	for _, run := range snapshot.Page.Runs {
		if run.Turn.String() != turn || run.Status != "running" || run.Waiting == nil || run.Waiting.Kind != "controller_login" {
			continue
		}
		root := x.r.localRuntime()
		waiting := run.Waiting
		// The credential is held either by this execution Node or by the
		// controller of a retained historical capture. The executor checks
		// the caller against the original turn's principal.
		credentialNode := waiting.Credential.Node
		if credentialNode != x.r.controller.Target() && credentialNode != root.controller.Target() {
			kit.Toast(x.c, kit.Failed, i18n.T(x.c, "controller_login_original_controller"))
			return
		}
		if waiting.Provider == (protocol.ProviderID{}) || waiting.Attempt == (protocol.RequestID{}) {
			break
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
		key := x.r.controller.Target().String() + "/" + run.Turn.String() + "/" + waiting.Attempt.String()
		recovery := root.authorizations.loginRecoveries[key]
		if recovery == nil {
			recovery = &controllerLoginRecovery{executor: x.r, input: protocol.CommandResumeControllerLoginPayload{
				Session: run.Session, Turn: run.Turn, Waiting: waiting.Attempt, Previous: waiting.Credential,
			}}
			root.authorizations.loginRecoveries[key] = recovery
		}
		// Re-pairing can replace the transport while the request remains open.
		// Keep its frozen identity, but use this Node's current Client.
		recovery.executor = x.r
		if recovery.busy || recovery.settled {
			return
		}
		if recovery.request != nil {
			root.restoreControllerLogin(recovery)
			return
		}
		if attempt := owner.authorizations.provider; attempt != nil && attempt.recovery == recovery && attempt.ctx.Err() == nil {
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
	recovery.input.Replacement = *replacement
	request := recovery.executor.controller.Prepare(protocol.Command{Kind: "resume_controller_login", Data: recovery.input})
	recovery.request = &request
	r.localRuntime().restoreControllerLogin(recovery)
}

func (r *Runtime) restoreControllerLogin(recovery *controllerLoginRecovery) {
	if recovery.busy || recovery.settled || recovery.request == nil {
		return
	}
	if r.controller != nil {
		if current := r.runtimeFor(recovery.request.Target.String()); current != nil {
			recovery.executor = current
		}
	}
	recovery.busy = true
	request := *recovery.request
	controller := recovery.executor.controller
	r.async(func() {
		_, err := controller.Settle(r.ctx, request)
		r.enqueue(func(c *ui.Context) {
			recovery.busy = false
			if err == nil {
				recovery.settled = true
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
			var fault *link.FaultError
			if errors.As(err, &fault) && fault.Fault.Details != nil && fault.Fault.Details.Kind == "controller_login" {
				if details, ok := detail[protocol.FaultDetailsControllerLoginPayload](fault.Fault.Details.Data); ok {
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
			// Keep the exact identity even for a final unknown outcome: Retry
			// inspects it and never starts a second recovery effect.
			if faultCode(err) != protocol.ErrorCodeOutcomeUnknown && faultCode(err) != protocol.ErrorCodeUnavailable && faultCode(err) != protocol.ErrorCodeCancelled {
				recovery.request = nil
			}
			var retry func()
			if recovery.request != nil {
				retry = func() { r.restoreControllerLogin(recovery) }
			}
			kit.Recover(c, outcome, i18n.T(c, key), retry)
		})
	})
}
