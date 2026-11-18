package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"slices"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type runtimeCapture struct {
	waiting bool
	// A peer predating capture reports keeps its existing access material
	// until expiry, removal or revocation. Never send it a fresh access token.
	legacy *protocol.ConfigurationFollowRecord
}

// Only a previously delivered capture can keep receiving a model credential.
func exportRuntimeCredentials(ctx context.Context, db *sql.Conn, controller protocol.NodeID, captured map[protocol.CredentialID]runtimeCapture) ([]protocol.ConfigurationFollowRecord, error) {
	result := []protocol.ConfigurationFollowRecord{}
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return nil, err
	}
	add := func(value protocol.RuntimeCredential) error {
		value.Controller = controller
		// Runtime projections include account metadata and package declarations
		// whose content can change independently of the credential's own CAS.
		record, err := compositeConfigurationRecord(ctx, db, user, "runtime_credentials", value.CacheID(), value.Revision, value)
		if err != nil {
			return err
		}
		value.Revision = record.Revision
		body, err := json.Marshal(value)
		if err != nil {
			return err
		}
		record.Body, record.Digest = body, ""
		record.ExpiresAtMs = value.ExpiresAtMs
		record.Digest, err = configurationDigest(record)
		if err == nil {
			result = append(result, record)
		}
		return err
	}
	if len(captured) != 0 {
		configs, err := providerConfigurations(ctx, db)
		if err != nil {
			return nil, err
		}
		for _, saved := range configs {
			if saved.Provider == nil {
				continue
			}
			var current *protocol.RuntimeCredential
			selected := saved.Provider.Credential
			if selected != nil && selected.Node == controller {
				if credential, ok := saved.Authorizations[selected.ID]; ok {
					candidate := runtimeProvider(*saved.Provider, selected.ID, credential)
					current = &candidate
				}
			}
			includeCurrent := false
			for id, credential := range saved.Authorizations {
				capture, ok := captured[id]
				if !ok {
					continue
				}
				value := runtimeProvider(*saved.Provider, id, credential)
				if capture.legacy != nil {
					if value.State == "ready" || value.State == "uncertain" {
						result = append(result, *capture.legacy)
					}
					continue
				}
				// A replacement is usable only for the same service-issued
				// account and authentication target as the original capture.
				if current != nil && selected.ID != id && value.Authentication != protocol.AuthenticationApiKey {
					reason := runtimeLoginRefusal(value, *current)
					if reason != "" {
						value.RefusedReplacement = &protocol.RuntimeLoginRefusal{ID: selected.ID, Reason: reason}
					} else if capture.waiting {
						includeCurrent = true
					}
				}
				if err := add(value); err != nil {
					return nil, err
				}
			}
			if includeCurrent {
				if _, alreadyCaptured := captured[selected.ID]; alreadyCaptured {
					continue
				}
				if err := add(*current); err != nil {
					return nil, err
				}
			}
		}
	}
	slices.SortFunc(result, func(a, b protocol.ConfigurationFollowRecord) int { return strings.Compare(a.ID, b.ID) })
	return result, nil
}

func runtimeLoginRefusal(original, replacement protocol.RuntimeCredential) string {
	switch {
	case original.AccountIdentity == "" || replacement.AccountIdentity == "" || original.AuthenticationTarget == "" || replacement.AuthenticationTarget == "":
		return "identity_unavailable"
	case original.AuthenticationTarget != replacement.AuthenticationTarget:
		return "target_changed"
	case original.AccountIdentity != replacement.AccountIdentity:
		return "account_mismatch"
	default:
		return ""
	}
}

func runtimeProvider(provider protocol.ConversationProvider, id protocol.CredentialID, credential providerAuthorization) protocol.RuntimeCredential {
	value := protocol.RuntimeCredential{
		Kind: "provider", ID: id.String(), Generation: id.String(), Revision: max(1, credential.Revision),
		Authentication: credential.Authentication, Provider: &provider.ID, State: "revoked",
	}
	if credential.Key == nil {
		return value
	}
	if credential.Authentication == protocol.AuthenticationApiKey || credential.Authentication == "" {
		value.State, value.Authentication, value.Access = "ready", protocol.AuthenticationApiKey, *credential.Key
		return value
	}
	var grant providers.AccountGrant
	if json.Unmarshal([]byte(credential.Key.Expose()), &grant) != nil {
		value.State = "unavailable"
		return value
	}
	exported, err := providers.RuntimeAccount(provider, grant)
	if err != nil {
		value.State = "unavailable"
		return value
	}
	exported.Kind, exported.ID, exported.Generation = value.Kind, value.ID, value.Generation
	exported.Revision, exported.State = value.Revision, "ready"
	if grant.RenewalPending || grant.RenewalUncertain {
		exported.State = "uncertain"
	}
	if grant.RenewalRejected {
		exported.State = "rejected"
	}
	exported.AccountIdentity = providers.RecoveryAccountIdentity(grant)
	target := credential.Auth
	if grant.Record != nil {
		target = &grant.Record.Target
	}
	exported.AuthenticationTarget = providers.RecoveryAuthenticationTarget(provider, target, grant)
	return exported
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
