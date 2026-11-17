package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"time"

	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func controllerLoginFault(reason string, code protocol.ErrorCode, message string) error {
	return &protocol.Fault{Code: code, Message: message, Details: &protocol.FaultDetails{
		Kind: "controller_login", Data: &protocol.FaultDetailsControllerLoginPayload{Reason: reason},
	}}
}

// ProviderRecoveryIdentity reads account identity even after expiry, without
// treating an expired token or an unresolved refresh as usable authorization.
func ProviderRecoveryIdentity(ctx context.Context, db *sql.Conn, node protocol.NodeID, provider protocol.ConversationProvider, reference protocol.CredentialRef) (string, string, error) {
	controller, err := CurrentController(ctx, db, node)
	if err != nil {
		return "", "", err
	}
	if reference.Node != controller {
		return "", "", controllerLoginFault("target_changed", protocol.ErrorCodePermissionDenied, "credential belongs to another controller")
	}
	if controller != node {
		value, err := readRuntimeCredential(ctx, db, node, "provider", reference.ID.String())
		if err != nil {
			return "", "", err
		}
		if value.Provider == nil || *value.Provider != provider.ID || value.Authentication != provider.Authentication || value.Generation != reference.ID.String() {
			return "", "", controllerLoginFault("target_changed", protocol.ErrorCodePermissionDenied, "credential does not match the captured provider")
		}
		return value.AccountIdentity, value.AuthenticationTarget, nil
	}
	saved, err := authorizationProvider(ctx, db, provider)
	if err != nil {
		return "", "", err
	}
	credential, exists := saved.Authorizations[reference.ID]
	if !exists || credential.Key == nil || credential.Authentication != provider.Authentication {
		return "", "", controllerLoginFault("identity_unavailable", protocol.ErrorCodeNotConfigured, "the original account identity is unavailable")
	}
	var grant providers.AccountGrant
	if err := json.Unmarshal([]byte(credential.Key.Expose()), &grant); err != nil {
		return "", "", err
	}
	target := credential.Auth
	if grant.Record != nil {
		target = &grant.Record.Target
	}
	return providers.RecoveryAccountIdentity(grant), providers.RecoveryAuthenticationTarget(provider, target, grant), nil
}

func capturedRunProvider(ctx context.Context, db *sql.Conn, turn protocol.TurnID) (protocol.ConversationProvider, error) {
	var body []byte
	var provider protocol.ConversationProvider
	if err := db.QueryRowContext(ctx, "SELECT provider FROM agent_runs WHERE turn=?1", turn.String()).Scan(&body); err != nil {
		return provider, err
	}
	err := json.Unmarshal(body, &provider)
	return provider, err
}

// CheckControllerLoginWait is repeated before admission and in the effect
// transaction. A shared project's visibility alone never grants the right to
// change another controller's captured account.
func CheckControllerLoginWait(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, input protocol.CommandResumeControllerLoginPayload) (protocol.ConversationRun, error) {
	var empty protocol.ConversationRun
	if input.Session == (protocol.SessionID{}) || input.Turn == (protocol.TurnID{}) || input.Waiting == (protocol.RequestID{}) || input.Previous == input.Replacement {
		return empty, controllerLoginFault("state_changed", protocol.ErrorCodeInvalidRequest, "an exact login wait and a new credential are required")
	}
	if err := CheckTurnAccess(ctx, db, input.Turn); err != nil {
		return empty, err
	}
	run, err := ReadAgentRun(ctx, db, input.Turn)
	if err != nil {
		return run, err
	}
	if run.Session != input.Session || run.Status != protocol.ConversationStatusRunning || run.Waiting == nil || run.Waiting.Kind != "controller_login" || run.Waiting.Attempt != input.Waiting || run.Waiting.Credential != input.Previous {
		return run, controllerLoginFault("state_changed", protocol.ErrorCodeRevisionConflict, "the task is no longer in this login wait")
	}
	origin, err := OriginalTurnUser(ctx, db, input.Turn)
	if err != nil {
		return run, err
	}
	owner, err := ResolveUser(ctx, db, origin.ID)
	if err != nil {
		return run, err
	}
	current, err := CurrentUser(ctx, db)
	if err != nil {
		return run, err
	}
	controller, err := CurrentController(WithUser(ctx, owner), db, node)
	if err != nil {
		return run, err
	}
	if current.ID != owner.ID || caller != controller || input.Previous.Node != controller || input.Replacement.Node != controller {
		return run, controllerLoginFault("target_changed", protocol.ErrorCodePermissionDenied, "only the task's original controller can restore its login")
	}
	return run, nil
}

// ControllerLoginReplacementAvailable allows the normal configuration push to
// arrive before a new durable recovery is admitted. No refresh material is
// fetched from the controller by the executor.
func ControllerLoginReplacementAvailable(ctx context.Context, db *sql.Conn, node protocol.NodeID, input protocol.CommandResumeControllerLoginPayload) (bool, error) {
	controller, err := CurrentController(ctx, db, node)
	if err != nil || controller == node {
		return err == nil, err
	}
	_, exists, err := ReadConfigurationCache(ctx, db, "runtime_credentials", "provider:"+input.Replacement.ID.String())
	return exists, err
}

type credentialRecovery struct {
	Request     protocol.RequestID     `json:"request"`
	Waiting     protocol.RequestID     `json:"waiting"`
	Previous    protocol.CredentialRef `json:"previous"`
	Replacement protocol.CredentialRef `json:"replacement"`
	ResumedAtMs int64                  `json:"resumed_at_ms"`
}

// EffectiveTurnCredential leaves the admission capture immutable. Only this
// run's future model steps consult the latest explicitly accepted recovery.
func EffectiveTurnCredential(ctx context.Context, db *sql.Conn, turn protocol.TurnID, captured protocol.CredentialRef) (protocol.CredentialRef, error) {
	var body []byte
	err := db.QueryRowContext(ctx, "SELECT body FROM agent_credential_recoveries WHERE turn=?1 ORDER BY sequence DESC LIMIT 1", turn.String()).Scan(&body)
	if errors.Is(err, sql.ErrNoRows) {
		return captured, nil
	}
	if err != nil {
		return captured, err
	}
	var recovery credentialRecovery
	if err := json.Unmarshal(body, &recovery); err != nil {
		return captured, err
	}
	return recovery.Replacement, nil
}

func resumeControllerLogin(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, request protocol.Request, now time.Time) (protocol.Output, []protocol.Event, error) {
	input := *request.Command.Data.(*protocol.CommandResumeControllerLoginPayload)
	run, err := CheckControllerLoginWait(ctx, db, node, caller, input)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	provider, err := capturedRunProvider(ctx, db, input.Turn)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if provider.ID != run.Waiting.Provider || provider.Authentication == protocol.AuthenticationApiKey {
		return protocol.Output{}, nil, controllerLoginFault("target_changed", protocol.ErrorCodePermissionDenied, "the task's captured authentication target changed")
	}
	if run.Waiting.AccountIdentity == "" || run.Waiting.AuthenticationTarget == "" {
		// A configuration push can enrich the original credential and deliver its
		// replacement before the waiting step has refreshed its projection.
		identity, target, err := ProviderRecoveryIdentity(ctx, db, node, provider, input.Previous)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if run.Waiting.AccountIdentity == "" {
			run.Waiting.AccountIdentity = identity
		}
		if run.Waiting.AuthenticationTarget == "" {
			run.Waiting.AuthenticationTarget = target
		}
	}
	identity, target, err := ProviderRecoveryIdentity(ctx, db, node, provider, input.Replacement)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if run.Waiting.AccountIdentity == "" || identity == "" || run.Waiting.AuthenticationTarget == "" || target == "" {
		return protocol.Output{}, nil, controllerLoginFault("identity_unavailable", protocol.ErrorCodeNotConfigured, "the service did not provide a trusted account identity for task recovery")
	}
	if target != run.Waiting.AuthenticationTarget {
		return protocol.Output{}, nil, controllerLoginFault("target_changed", protocol.ErrorCodePermissionDenied, "the new login uses a different authentication target")
	}
	if identity != run.Waiting.AccountIdentity {
		return protocol.Output{}, nil, controllerLoginFault("account_mismatch", protocol.ErrorCodePermissionDenied, "sign in with the task's original account and workspace")
	}
	if _, err := ReadProviderAuthorization(ctx, db, node, provider, input.Replacement); err != nil {
		return protocol.Output{}, nil, err
	}
	recovery := credentialRecovery{Request: request.ID, Waiting: input.Waiting, Previous: input.Previous, Replacement: input.Replacement, ResumedAtMs: now.UnixMilli()}
	body, err := encode(recovery)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if _, err := db.ExecContext(ctx, "INSERT INTO agent_credential_recoveries(turn,request,body) VALUES(?1,?2,?3)", input.Turn.String(), request.ID.String(), body); err != nil {
		return protocol.Output{}, nil, err
	}
	run.Waiting = nil
	if err := putAgentRun(ctx, db, run); err != nil {
		return protocol.Output{}, nil, err
	}
	events, err := activityEvent(ctx, db, run.Session)
	return protocol.Output{Kind: "run", Data: run}, events, err
}
