package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"slices"
	"strconv"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/node/authorization"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// ExportRuntimeCredentials is deliberately separate from configuration export:
// a full account grant or connector OAuth configuration must never be copied.
func ExportRuntimeCredentials(ctx context.Context, db *sql.Conn, controller protocol.NodeID) ([]protocol.ConfigurationFollowRecord, error) {
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
	saved, err := providerConfigurations(ctx, db)
	if err != nil {
		return nil, err
	}
	for _, saved := range saved {
		if saved.Provider == nil {
			continue
		}
		for id, credential := range saved.Authorizations {
			value := protocol.RuntimeCredential{
				Kind: "provider", ID: id.String(), Generation: id.String(), Revision: max(1, credential.Revision),
				Authentication: credential.Authentication, Provider: &saved.Provider.ID,
				State: "revoked",
			}
			if credential.Key != nil {
				value.State = "ready"
				if credential.Authentication == protocol.AuthenticationApiKey || credential.Authentication == "" {
					value.Authentication, value.Access = protocol.AuthenticationApiKey, *credential.Key
				} else {
					var grant providers.AccountGrant
					if json.Unmarshal([]byte(credential.Key.Expose()), &grant) != nil {
						value.State = "unavailable"
					} else {
						exported, exportErr := providers.RuntimeAccount(*saved.Provider, grant)
						if exportErr != nil {
							value.State = "unavailable"
						} else {
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
							exported.AuthenticationTarget = providers.RecoveryAuthenticationTarget(*saved.Provider, target, grant)
							value = exported
						}
					}
				}
			}
			if err := add(value); err != nil {
				return nil, err
			}
		}
	}
	connections, err := connectorRecords(ctx, db)
	if err != nil {
		return nil, err
	}
	for _, record := range connections {
		value, err := runtimeConnector(ctx, db, record)
		if err != nil {
			return nil, err
		}
		if err := add(value); err != nil {
			return nil, err
		}
	}
	slices.SortFunc(result, func(a, b protocol.ConfigurationFollowRecord) int { return strings.Compare(a.ID, b.ID) })
	return result, nil
}

func runtimeConnector(ctx context.Context, db *sql.Conn, record ConnectorRecord) (protocol.RuntimeCredential, error) {
	value := protocol.RuntimeCredential{
		Kind: "connector", ID: record.ID.String(), Generation: strconv.FormatUint(record.AuthRevision, 10),
		Revision: max(1, record.Revision), State: "ready", Slots: map[string]protocol.Secret{},
	}
	switch {
	case record.Method == protocol.PluginConnectorKindLocalCli:
		value.State = "machine_bound"
	case !record.Enabled || record.Login != nil:
		value.State = "revoked"
	case record.Method != protocol.PluginConnectorKindSecret && record.Grant == nil:
		value.State = "revoked"
	case record.Grant != nil:
		value.Access, value.ExpiresAtMs = protocol.NewSecret(record.Grant.Access), record.Grant.ExpiresAtMs
		value.IssuedAtMs = record.Grant.IssuedAtMs
		if record.Grant.Extra["renewal_uncertain"] == "true" {
			value.State = "uncertain"
		}
		if record.Grant.Extra["renewal_rejected"] == "true" {
			value.State = "rejected"
		}
	}
	if value.State == "machine_bound" || value.State == "revoked" {
		return value, nil
	}
	info, err := ReadPlugin(ctx, db, record.Plugin)
	if err != nil {
		return value, err
	}
	if info == nil {
		value.State = "unavailable"
		return value, nil
	}
	for _, declaration := range ConnectorDeclarations(*info) {
		if declaration.ID != record.Connector {
			continue
		}
		for _, injection := range declaration.Inject {
			if injection.Field == nil {
				continue
			}
			name := *injection.Field
			if !providers.RuntimeField(name) {
				value.State = "unavailable"
				continue
			}
			if secret, ok := record.Secrets[name]; ok {
				value.Slots[name] = secret
			} else if text, ok := record.Fields[name]; ok {
				value.Slots[name] = protocol.NewSecret(text)
			}
		}
		// The implicit first field of secret authentication is also an
		// execution slot when an injection omits its field selection.
		if record.Method == protocol.PluginConnectorKindSecret {
			for _, auth := range declaration.Auth {
				if auth.Kind != string(record.Method) {
					continue
				}
				var selection protocol.PluginConnectorAuthSecretPayload
				body, _ := json.Marshal(auth.Data)
				if json.Unmarshal(body, &selection) == nil && len(selection.Fields) > 0 && providers.RuntimeField(selection.Fields[0]) {
					name := selection.Fields[0]
					if secret, ok := record.Secrets[name]; ok {
						value.Slots[name] = secret
					}
				}
			}
		}
		return value, nil
	}
	value.State = "unavailable"
	return value, nil
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
		return value, fault(protocol.ErrorCodeNotConfigured, "controller runtime credential is unavailable")
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
		return fault(protocol.ErrorCodeNotConfigured, "controller credential was revoked")
	case "machine_bound":
		return fault(protocol.ErrorCodeNotConfigured, "authentication is bound to the controller machine")
	default:
		return fault(protocol.ErrorCodeNotConfigured, "controller runtime credential is unavailable")
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

func ReadRuntimeConnectorCredential(ctx context.Context, db *sql.Conn, node protocol.NodeID, record ConnectorRecord) (*protocol.Secret, error) {
	if !record.Enabled || record.Login != nil {
		return nil, fault(protocol.ErrorCodeNotConfigured, "controller connector authorization is unavailable")
	}
	value, err := readRuntimeCredential(ctx, db, node, "connector", record.ID.String())
	if err != nil {
		return nil, err
	}
	if value.Generation != strconv.FormatUint(record.AuthRevision, 10) {
		return nil, fault(protocol.ErrorCodeRevisionConflict, "connector account changed")
	}
	if err := runtimeCredentialReady(value); err != nil {
		return nil, err
	}
	if record.Method == protocol.PluginConnectorKindSecret {
		return nil, nil
	}
	token := value.Access
	return &token, nil
}

// restoreRuntimeConnector only reconstructs the slots used by execution.
// OAuth refresh data and the provider client configuration stay absent.
func restoreRuntimeConnector(ctx context.Context, db *sql.Conn, record *ConnectorRecord) error {
	body, exists, err := ReadConfigurationCache(ctx, db, "runtime_credentials", "connector:"+record.ID.String())
	if err != nil || !exists {
		return err
	}
	var value protocol.RuntimeCredential
	if err := json.Unmarshal(body, &value); err != nil {
		return err
	}
	if value.Kind != "connector" || value.ID != record.ID.String() || value.Generation != strconv.FormatUint(record.AuthRevision, 10) {
		return nil
	}
	record.Secrets = value.Slots
	if value.State == "ready" || value.State == "uncertain" {
		record.Grant = &authorization.Issued{Access: value.Access.Expose(), ExpiresAtMs: value.ExpiresAtMs, IssuedAtMs: value.IssuedAtMs}
	}
	return nil
}
