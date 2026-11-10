package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"math"
	"sort"
	"time"

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
func executeProviderCredential(ctx context.Context, db *sql.Conn, node protocol.NodeID, command protocol.Command) (protocol.Output, []protocol.Event, error) {
	var id protocol.CredentialID
	var expected uint64
	var provider protocol.ProviderID
	var secret *protocol.Secret
	var expiry *uint64
	if command.Kind == "put_credential" {
		input, err := providerPayload[protocol.CommandPutCredentialPayload](command)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		id, expected, provider, secret, expiry = input.ID, input.ExpectedRevision, input.Provider, &input.Secret, input.ExpiresAtMs
		if err := validateProviderKey(*secret); err != nil {
			return protocol.Output{}, nil, err
		}
		if expiry != nil && (*expiry > math.MaxInt64 || *expiry <= uint64(time.Now().UnixMilli())) {
			return protocol.Output{}, nil, invalid("credential expiry must be in the future")
		}
	} else {
		input, err := providerPayload[protocol.CommandRevokeCredentialPayload](command)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		id, expected = input.ID, input.ExpectedRevision
	}
	saved, err := readBody[providerConfiguration](ctx, db, "SELECT body FROM providers WHERE json_type(body,?1) IS NOT NULL", `$.authorizations."`+id.String()+`"`)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	var previous providerAuthorization
	revision := uint64(0)
	if saved != nil {
		previous = saved.Authorizations[id]
		revision = previous.Revision
		if command.Kind == "put_credential" && (saved.ID != provider || previous.Authentication != protocol.AuthenticationApiKey) {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeConflict, "credential provider and authentication cannot change")
		}
		provider = saved.ID
	} else {
		if command.Kind == "revoke_credential" {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeNotFound, "credential not found")
		}
		if err := providerCredentialCapacity(ctx, db); err != nil {
			return protocol.Output{}, nil, err
		}
		saved, err = readBody[providerConfiguration](ctx, db, "SELECT body FROM providers WHERE id=?1", provider.String())
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if saved == nil {
			saved = &providerConfiguration{ID: provider, Authorizations: map[protocol.CredentialID]providerAuthorization{}}
		}
	}
	if err := checkRevision(revision, expected); err != nil {
		return protocol.Output{}, nil, err
	}
	revision, err = nextRevision(revision)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if saved.Authorizations == nil {
		saved.Authorizations = map[protocol.CredentialID]providerAuthorization{}
	}
	value := providerAuthorization{Authentication: protocol.AuthenticationApiKey, Revision: revision, Key: secret, ExpiresAtMs: expiry}
	if command.Kind == "revoke_credential" {
		value = previous
		value.Revision = revision
		value.Key = nil
	}
	saved.Authorizations[id] = value
	if err := saveProviderConfiguration(ctx, db, *saved); err != nil {
		return protocol.Output{}, nil, err
	}
	metadata := providerCredentialMetadata(provider, id, value)
	return protocol.Output{Kind: "credential", Data: metadata}, []protocol.Event{{Kind: "credential_changed", Data: protocol.EventCredentialChangedPayload{Credential: protocol.CredentialRef{Node: node, ID: id}, Revision: revision}}}, nil
}
