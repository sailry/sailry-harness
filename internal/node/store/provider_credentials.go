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
	saved, err := providerConfigurations(ctx, db)
	if err != nil {
		return nil, err
	}
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return nil, err
	}
	result := []protocol.Credential{}
	for _, provider := range saved {
		for id, value := range provider.Authorizations {
			if provider.Provider != nil && provider.Provider.Shared && !user.Administrator && (provider.Provider.Credential == nil || provider.Provider.Credential.ID != id) {
				continue
			}
			result = append(result, providerCredentialMetadata(provider.ID, id, value))
		}
	}
	sort.Slice(result, func(i, j int) bool { return result[i].ID.String() < result[j].ID.String() })
	return result, nil
}

// Capacity bounds current provider selections, not immutable authorizations
// retained by captured work. An unlisted provider counts while a live session
// uses it, including session-private providers imported from another Node.
func providerCredentialCapacity(ctx context.Context, db *sql.Conn, replacing protocol.ProviderID, additional int) error {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	owner, present, err := configurationOwner(ctx, db, "providers", replacing.String())
	if err != nil {
		return err
	}
	if present {
		user.ID = owner
	}
	var count int
	err = db.QueryRowContext(ctx, `SELECT count(*) FROM providers p JOIN json_each(p.body,'$.authorizations') a
 ON a.key=json_extract(p.body,'$.provider.credential.id')
 WHERE p.id!=?1 AND COALESCE(p.user,(SELECT id FROM users WHERE local=1))=?2 AND json_extract(a.value,'$.key') IS NOT NULL
 AND (json_extract(p.body,'$.listed') OR EXISTS(
 SELECT 1 FROM sessions s JOIN session_revisions r ON r.session=s.id AND r.revision=s.revision
 WHERE s.state!='removed' AND (json_extract(r.config,'$.provider')=p.id
 OR json_extract(r.profile,'$.provider.id')=p.id
 OR EXISTS(SELECT 1 FROM json_each(r.roles,'$.providers') source WHERE json_extract(source.value,'$.id')=p.id))))`, replacing.String(), user.ID.String()).Scan(&count)
	if err != nil {
		return err
	}
	if count+additional > 256 {
		return fault(protocol.ErrorCodeBusy, "credential capacity exhausted")
	}
	return nil
}
