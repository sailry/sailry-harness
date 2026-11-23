package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"net/http"
	"sync/atomic"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/oauth2"
)

func runtimeCredentialFixture(t *testing.T) (*Node, context.Context, protocol.ConversationProvider, protocol.CredentialRef, func(protocol.RuntimeCredential)) {
	t.Helper()
	node := nodeFixture(t)
	controller := protocol.NodeID{39}
	var user protocol.User
	err := node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		var err error
		user, err = store.CreateUser(t.Context(), db, "Runtime controller")
		if err != nil {
			return err
		}
		_, err = db.ExecContext(t.Context(), "UPDATE users SET controller=?2 WHERE id=?1", user.ID.String(), controller[:])
		return err
	})
	if err != nil {
		t.Fatal(err)
	}
	user.Controller = &controller
	ctx := store.WithUser(t.Context(), user)
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Authentication = protocol.NewProviderID(), protocol.AuthenticationClaudeOAuth
	provider.Api, provider.Endpoint = protocol.ConversationModelApiAnthropic, "https://api.anthropic.com"
	reference := protocol.CredentialRef{Node: controller, ID: protocol.NewCredentialID()}
	write := func(value protocol.RuntimeCredential) {
		t.Helper()
		value.Controller = controller
		body, err := json.Marshal(value)
		if err != nil {
			t.Fatal(err)
		}
		err = node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
			_, err := db.ExecContext(ctx, `INSERT INTO configuration_cache(user,category,id,revision,digest,body)
VALUES(?1,'runtime_credentials',?2,?3,'runtime-fixture',?4) ON CONFLICT(user,category,id)
DO UPDATE SET body=excluded.body,revision=excluded.revision`,
				user.ID.String(), value.CacheID(), value.Revision, body)
			return err
		})
		if err != nil {
			t.Fatal(err)
		}
		node.RuntimeCredentialsChanged()
	}
	return node, ctx, provider, reference, write
}

func TestExecutorWaitResumesOnlyUnsentSameAccountStep(t *testing.T) {
	t.Parallel()
	node, ctx, provider, reference, write := runtimeCredentialFixture(t)
	var requests atomic.Int32
	node.auth.client = &http.Client{Transport: accountHTTP(func(*http.Request) (*http.Response, error) {
		requests.Add(1)
		return nil, errors.New("an executor must never refresh")
	})}
	expired, future := uint64(1), uint64(time.Now().Add(time.Hour).UnixMilli())
	value := protocol.RuntimeCredential{
		Kind: "provider", ID: reference.ID.String(), Generation: reference.ID.String(), Revision: 1, State: "ready",
		Provider: &provider.ID, Authentication: provider.Authentication, Endpoint: provider.Endpoint,
		Access: protocol.NewSecret("expired-runtime"), ExpiresAtMs: &expired,
	}
	write(value)
	if _, err := node.providerAuthorization(ctx, provider, reference); err == nil {
		t.Fatal("ordinary plugin authorization waited or accepted an expired token")
	}
	transitions := []bool{}
	secret, _, err := node.awaitProviderAuthorization(ctx, provider, func() (protocol.CredentialRef, error) { return reference, nil }, func(_ protocol.CredentialRef, waiting bool) error {
		transitions = append(transitions, waiting)
		if waiting {
			// Commit while the wait projection is being published. The wake
			// precedes the select and still resumes this exact model step.
			value.Access, value.ExpiresAtMs, value.Revision = protocol.NewSecret("fresh-runtime"), &future, 2
			write(value)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	grant, err := providers.DecodeAccount(secret, provider.Authentication)
	if err != nil || grant.Access != "fresh-runtime" || len(transitions) != 2 || !transitions[0] || transitions[1] || requests.Load() != 0 {
		t.Fatalf("same account did not resume without refresh: %v, transitions=%v, requests=%d", err, transitions, requests.Load())
	}
}

func TestExecutorCredentialWaitCancelsAndRejectsChangedAccount(t *testing.T) {
	t.Parallel()
	for _, fixture := range []string{"cancel", "changed generation", "revoked"} {
		t.Run(fixture, func(t *testing.T) {
			node, owner, provider, reference, write := runtimeCredentialFixture(t)
			ctx, cancel := context.WithCancel(owner)
			defer cancel()
			expired := uint64(1)
			value := protocol.RuntimeCredential{
				Kind: "provider", ID: reference.ID.String(), Generation: reference.ID.String(), Revision: 1, State: "ready",
				Provider: &provider.ID, Authentication: provider.Authentication, Endpoint: provider.Endpoint,
				Access: protocol.NewSecret("expired-runtime"), ExpiresAtMs: &expired,
			}
			write(value)
			cleared := false
			_, _, err := node.awaitProviderAuthorization(ctx, provider, func() (protocol.CredentialRef, error) { return reference, nil }, func(_ protocol.CredentialRef, waiting bool) error {
				if !waiting {
					cleared = true
					return nil
				}
				switch fixture {
				case "cancel":
					cancel()
				case "changed generation":
					value.Generation = protocol.NewCredentialID().String()
					value.Revision++
					write(value)
				case "revoked":
					value.State, value.Revision = "revoked", value.Revision+1
					write(value)
				}
				return nil
			})
			if err == nil || !cleared {
				t.Fatalf("wait survived cancellation or account replacement: %v, cleared=%v", err, cleared)
			}
			if fixture == "cancel" && !errors.Is(err, context.Canceled) {
				t.Fatalf("wait cancellation=%v", err)
			}
		})
	}
}

func TestPairedNodeMCPSourceUsesItsOwnAuthorization(t *testing.T) {
	t.Parallel()
	node, ctx, _, _, write := runtimeCredentialFixture(t)
	record := store.ConnectorRecord{
		ID: protocol.NewRequestID(), Plugin: "fixture", Connector: "account", Method: protocol.PluginConnectorKindMcpOauth,
		Revision: 1, AuthRevision: 4, GrantRevision: 1, Enabled: true, State: protocol.PluginConnectorStateConnected,
		Fields: map[string]string{}, Secrets: map[string]protocol.Secret{},
	}
	expiration := time.Now().Add(time.Hour)
	expiry := uint64(expiration.UnixMilli())
	write(protocol.RuntimeCredential{
		Kind: "connector", ID: record.ID.String(), Generation: "4", Revision: 3, State: "ready",
		Access: protocol.NewSecret("runtime-mcp-access"), ExpiresAtMs: &expiry,
	})
	var fault *protocol.Fault
	if _, err := node.connectorMCPSource(record, ctx); !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeNotConfigured {
		t.Fatalf("controller cache supplied connector authorization: %v", err)
	}
	local := connectorOAuthIssued(&oauth2.Config{ClientID: "fixture"}, &oauth2.Token{AccessToken: "node-mcp-access", Expiry: expiration})
	record.Grant = &local
	body, err := json.Marshal(record)
	if err != nil {
		t.Fatal(err)
	}
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		user, err := store.CurrentUser(ctx, db)
		if err != nil {
			return err
		}
		_, err = db.ExecContext(ctx, "INSERT INTO plugin_connections(id,plugin,connector,revision,body,user) VALUES(?1,?2,?3,?4,?5,?6)",
			record.ID.String(), record.Plugin, record.Connector, record.Revision, body, user.ID.String())
		return err
	}); err != nil {
		t.Fatal(err)
	}
	source, err := node.connectorMCPSource(record, ctx)
	if err != nil {
		t.Fatal(err)
	}
	token, err := source.Token()
	if err != nil || token == nil || token.AccessToken != "node-mcp-access" {
		t.Fatalf("paired Node could not use its MCP authorization: %v", err)
	}
}
