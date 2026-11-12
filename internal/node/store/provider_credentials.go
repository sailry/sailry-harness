package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"sort"

	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Existing current-format rows predate authentication metadata. Their ordinary
// inline API key retains the same owner when that metadata becomes explicit.
func (value *providerAuthorization) UnmarshalJSON(data []byte) error {
	type plain providerAuthorization
	decoded := plain{Authentication: protocol.AuthenticationApiKey, Revision: 1}
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	*value = providerAuthorization(decoded)
	return nil
}
func (value providerAuthorization) MarshalJSON() ([]byte, error) {
	type plain providerAuthorization
	if value.Authentication == "" {
		value.Authentication = protocol.AuthenticationApiKey
	}
	if value.Revision == 0 {
		value.Revision = 1
	}
	return json.Marshal(plain(value))
}
func providerCredentialMetadata(provider protocol.ProviderID, id protocol.CredentialID, saved providerAuthorization) protocol.Credential {
	authentication := saved.Authentication
	if authentication == "" {
		authentication = protocol.AuthenticationApiKey
	}
	revision := saved.Revision
	if revision == 0 {
		revision = 1
	}
	metadata := protocol.Credential{ID: id, Provider: provider, Authentication: authentication, Revision: revision, ExpiresAtMs: saved.ExpiresAtMs, Revoked: saved.Key == nil}
	if authentication != protocol.AuthenticationApiKey && saved.Key != nil {
		var grant providers.AccountGrant
		if json.Unmarshal([]byte(saved.Key.Expose()), &grant) == nil {
			metadata.AccountLabel = grant.Label()
		}
	}
	return metadata
}
func providerCredentials(ctx context.Context, db *sql.Conn) ([]protocol.Credential, error) {
	saved, err := readBodies[providerConfiguration](ctx, db, "SELECT body FROM providers ORDER BY rowid")
	if err != nil {
		return nil, err
	}
	result := []protocol.Credential{}
	for _, provider := range saved {
		for id, value := range provider.Authorizations {
			result = append(result, providerCredentialMetadata(provider.ID, id, value))
		}
	}
	sort.Slice(result, func(i, j int) bool { return result[i].ID.String() < result[j].ID.String() })
	return result, nil
}
func providerCredentialCapacity(ctx context.Context, db *sql.Conn) error {
	var count int
	if err := db.QueryRowContext(ctx, "SELECT count(*) FROM providers,json_each(providers.body,'$.authorizations')").Scan(&count); err != nil {
		return err
	}
	if count >= 256 {
		return fault(protocol.ErrorCodeBusy, "credential capacity exhausted")
	}
	return nil
}
