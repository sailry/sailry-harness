package store

import (
	"context"
	"database/sql"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// RecoverCredentialRenewals never repeats an exchange whose response may have
// been lost. Its access token remains usable until expiry; explicit sign-in is
// needed afterward. Publish a new revision for the changed execution state.
func RecoverCredentialRenewals(ctx context.Context, db *sql.Conn, node protocol.NodeID) error {
	users, err := ListUsers(ctx, db)
	if err != nil {
		return err
	}
	for _, user := range users {
		if user.State != "active" {
			continue
		}
		owner := WithUser(ctx, user)
		controller, err := CurrentController(owner, db, node)
		if err != nil {
			return err
		}
		if controller != node {
			continue
		}
		configurations, err := providerConfigurations(owner, db)
		if err != nil {
			return err
		}
		for _, saved := range configurations {
			changed := false
			for id, value := range saved.Authorizations {
				if value.Key == nil || value.Authentication == protocol.AuthenticationApiKey {
					continue
				}
				var grant providers.AccountGrant
				if err := json.Unmarshal([]byte(value.Key.Expose()), &grant); err != nil || !grant.RenewalPending {
					continue
				}
				grant.RenewalPending, grant.RenewalUncertain = false, true
				body, err := json.Marshal(grant)
				if err != nil {
					return err
				}
				secret := protocol.NewSecret(string(body))
				value.Key = &secret
				value.Revision, err = nextRevision(value.Revision)
				if err != nil {
					return err
				}
				saved.Authorizations[id] = value
				changed = true
			}
			if changed {
				if err := saveProviderConfiguration(owner, db, saved); err != nil {
					return err
				}
			}
		}
		connections, err := connectorRecords(owner, db)
		if err != nil {
			return err
		}
		for _, record := range connections {
			if record.Grant == nil || record.Grant.Extra["renewal_pending"] != "true" {
				continue
			}
			delete(record.Grant.Extra, "renewal_pending")
			record.Grant.Extra["renewal_uncertain"] = "true"
			record.Revision, err = nextRevision(record.Revision)
			if err != nil {
				return err
			}
			record.GrantRevision, err = nextRevision(record.GrantRevision)
			if err != nil {
				return err
			}
			if err := saveConnectorRecord(owner, db, record); err != nil {
				return err
			}
		}
	}
	return nil
}
