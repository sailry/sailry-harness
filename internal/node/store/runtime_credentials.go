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

func RuntimeCredentialEvent(event string) bool {
	switch event {
	case "provider_changed", "provider_removed", "credential_changed",
		"connections_changed", "plugin_changed", "plugin_removed":
		return true
	default:
		return false
	}
}

func readRuntimeCredential(ctx context.Context, db *sql.Conn, node protocol.NodeID, kind, id string) (protocol.RuntimeCredential, error) {
	var value protocol.RuntimeCredential
	controller, err := CurrentController(ctx, db, node)
	if err != nil {
		return value, err
	}
	body, exists, err := ReadConfigurationCache(ctx, db, "runtime_credentials", kind+":"+id)
	if err != nil {
		return value, err
	}
	if !exists {
		return value, protocol.NewConfigurationFault(protocol.ConfigurationRuntimeCredentialMissing, "controller runtime credential is unavailable")
	}
	if err := json.Unmarshal(body, &value); err != nil {
		return value, err
	}
	if value.Controller != controller || value.Kind != kind || value.ID != id || value.Revision == 0 {
		return value, fault(protocol.ErrorCodePermissionDenied, "runtime credential belongs to a different controller or identity")
	}
	return value, nil
}

func runtimeCredentialReady(value protocol.RuntimeCredential) error {
	switch value.State {
	case "ready":
	case "uncertain":
		// An in-flight refresh does not invalidate the access token already
		// issued. Once expired, its unresolved outcome requires fresh login.
		if value.ExpiresAtMs == nil || *value.ExpiresAtMs <= uint64(time.Now().UnixMilli()) {
			return controllerLoginFault("renewal_uncertain", protocol.ErrorCodeOutcomeUnknown, "controller credential renewal is unresolved; sign in again")
		}
	case "rejected":
		return controllerLoginFault("renewal_rejected", protocol.ErrorCodePermissionDenied, "controller credential renewal was rejected; sign in again")
	case "revoked":
		return protocol.NewConfigurationFault(protocol.ConfigurationCredentialRevoked, "controller credential was revoked")
	case "machine_bound":
		return protocol.NewConfigurationFault(protocol.ConfigurationMachineBound, "authentication is bound to the controller machine")
	default:
		return protocol.NewConfigurationFault(protocol.ConfigurationRuntimeCredentialMissing, "controller runtime credential is unavailable")
	}
	if value.ExpiresAtMs != nil && *value.ExpiresAtMs <= uint64(time.Now().UnixMilli()) {
		return fault(protocol.ErrorCodeExpired, "controller runtime credential expired; waiting for the same account")
	}
	return nil
}

func expiredRuntimeCredential(err error) bool {
	var failure *protocol.Fault
	return errors.As(err, &failure) && failure.Code == protocol.ErrorCodeExpired
}

func ReadRuntimeProviderCredential(ctx context.Context, db *sql.Conn, node protocol.NodeID, provider protocol.ConversationProvider, reference protocol.CredentialRef) (*protocol.Secret, error) {
	value, err := readRuntimeCredential(ctx, db, node, "provider", reference.ID.String())
	if err != nil {
		return nil, err
	}
	if reference.Node != value.Controller || value.Generation != reference.ID.String() || value.Provider == nil || *value.Provider != provider.ID || value.Authentication != provider.Authentication {
		return nil, fault(protocol.ErrorCodePermissionDenied, "runtime credential does not match the captured account")
	}
	if err := runtimeCredentialReady(value); err != nil {
		return nil, err
	}
	if provider.Authentication == protocol.AuthenticationApiKey {
		key := value.Access
		return &key, nil
	}
	secret, err := providers.RuntimeAccountSecret(value)
	if err != nil {
		return nil, err
	}
	if _, _, err := providers.AccountHeaders(provider, secret); err != nil {
		return nil, err
	}
	return secret, nil
}
