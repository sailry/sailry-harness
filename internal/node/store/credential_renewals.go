package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type CredentialRenewal struct {
	User        protocol.User
	Provider    *protocol.ConversationProvider
	Credential  protocol.CredentialRef
	Connection  *ConnectorRecord
	Declaration protocol.PluginConnectorDeclaration
	Revision    uint64
	DueMs       uint64
}

// ErrProviderRenewalNotSelected means the account stopped being eligible
// after the scheduler collected it.
var ErrProviderRenewalNotSelected = errors.New("scheduled provider authorization is no longer selected")

func selectedProviderCredential(saved providerConfiguration, node protocol.NodeID) *protocol.CredentialRef {
	if !saved.Listed || saved.Provider == nil || !saved.Provider.Enabled {
		return nil
	}
	reference := saved.Provider.Credential
	if reference == nil || reference.Node != node {
		return nil
	}
	return reference
}

// ReadCredentialRenewals schedules currently used accounts owned here. A turn
// that has retained another account can renew it at its next model step.
func ReadCredentialRenewals(ctx context.Context, db *sql.Conn, node protocol.NodeID) ([]CredentialRenewal, error) {
	users, err := ListUsers(ctx, db)
	if err != nil {
		return nil, err
	}
	result := []CredentialRenewal{}
	for _, user := range users {
		if user.State != "active" {
			continue
		}
		owner := WithUser(ctx, user)
		configurations, err := providerConfigurations(owner, db)
		if err != nil {
			return nil, err
		}
		for _, saved := range configurations {
			reference := selectedProviderCredential(saved, node)
			if reference == nil {
				continue
			}
			value := saved.Authorizations[reference.ID]
			if value.Key == nil || value.Authentication == protocol.AuthenticationApiKey {
				continue
			}
			var grant providers.AccountGrant
			if json.Unmarshal([]byte(value.Key.Expose()), &grant) != nil || grant.RenewalPending || grant.RenewalUncertain || grant.RenewalRejected {
				continue
			}
			due := grant.RenewalAt()
			if due == nil {
				continue
			}
			captured, err := CaptureProvider(owner, db, *saved.Provider)
			if err != nil {
				continue
			}
			result = append(result, CredentialRenewal{
				User: user, Provider: &captured, Credential: *reference,
				Revision: value.Revision, DueMs: *due,
			})
		}
		connections, err := connectorRecords(owner, db)
		if err != nil {
			return nil, err
		}
		for _, record := range connections {
			if !record.Enabled || record.Login != nil || record.Grant == nil || record.Method == protocol.PluginConnectorKindLocalCli ||
				record.Grant.Extra["renewal_pending"] == "true" || record.Grant.Extra["renewal_uncertain"] == "true" || record.Grant.Extra["renewal_rejected"] == "true" {
				continue
			}
			due := ConnectorRenewalAt(record)
			if due == nil {
				continue
			}
			info, err := ReadPlugin(owner, db, record.Plugin)
			if err != nil {
				return nil, err
			}
			if info == nil || !info.Summary.Enabled {
				continue
			}
			for _, declaration := range ConnectorDeclarations(*info) {
				if declaration.ID == record.Connector {
					record := record
					result = append(result, CredentialRenewal{User: user, Connection: &record, Declaration: declaration, Revision: record.GrantRevision, DueMs: *due})
					break
				}
			}
		}
	}
	return result, nil
}

func ConnectorRenewalAt(record ConnectorRecord) *uint64 {
	if record.Grant == nil || record.Grant.ExpiresAtMs == nil {
		return nil
	}
	if (record.Method == protocol.PluginConnectorKindOauth2 || record.Method == protocol.PluginConnectorKindMcpOauth) && record.Grant.Refresh == nil {
		return nil
	}
	expiry, issued := *record.Grant.ExpiresAtMs, record.Grant.IssuedAtMs
	// Connector grants previously renewed only after expiry.
	due := expiry
	if issued != nil && *issued < expiry {
		due = *issued + (expiry-*issued)/4
	}
	return &due
}
