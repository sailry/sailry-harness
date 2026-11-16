package store

import (
	"context"
	"database/sql"
	"encoding/json"

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

// ReadCredentialRenewals includes only credentials whose renewal material is
// owned here. An executor's cache is never considered a renewal source.
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
		controller, err := CurrentController(owner, db, node)
		if err != nil {
			return nil, err
		}
		if controller != node {
			continue
		}
		configurations, err := providerConfigurations(owner, db)
		if err != nil {
			return nil, err
		}
		for _, saved := range configurations {
			if saved.Provider == nil {
				continue
			}
			for id, value := range saved.Authorizations {
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
					User: user, Provider: &captured, Credential: protocol.CredentialRef{Node: node, ID: id},
					Revision: value.Revision, DueMs: *due,
				})
			}
		}
		connections, err := connectorRecords(owner, db)
		if err != nil {
			return nil, err
		}
		for _, record := range connections {
			if !record.Enabled || record.Login != nil || record.Grant == nil || record.Method == protocol.PluginConnectorKindLocalCli ||
				record.Grant.Extra["renewal_pending"] == "true" || record.Grant.Extra["renewal_rejected"] == "true" {
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
	if record.Method == protocol.PluginConnectorKindOauth2 && record.Grant.Refresh == nil {
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
