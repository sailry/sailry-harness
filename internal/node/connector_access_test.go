package node

import (
	"database/sql"
	"errors"
	"io"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/authorization"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/oauth2"
)

// TestConnectorRenewal renews an expired OAuth grant against a token
// endpoint that cannot be reached, refuses, answers nothing definite, and
// finally issues a new grant.
func TestConnectorRenewal(t *testing.T) {
	node := nodeFixture(t)
	ctx := t.Context()
	source := t.TempDir()
	if err := os.WriteFile(filepath.Join(source, "plugin.json"), []byte(`{"$schema":"`+plugins.ManifestSchema+`","name":"renewal-fixture","version":"0.1.0","extensions":{"dev.sailry.platform":{"api_version":"v1","actions":[],"connectors":[{"id":"account","display":{"label":"Fixture account"},"accounts":"single","origins":["https://fixture.test"],"auth":[{"kind":"oauth2","flow":"pkce","authorize":"https://fixture.test/authorize","token":"https://fixture.test/token","client":{"fields":{"client_id":"client_id"}},"scopes":["read"],"refresh":true}],"fields":[{"name":"client_id","title":"Client ID","secret":false}],"inject":[{"origin":"https://fixture.test","header":"Authorization","prefix":"Bearer "}]}]}}}`), 0o600); err != nil {
		t.Fatal(err)
	}
	info, err := node.plugins.manager.Install(ctx, source, "", "renewal-fixture")
	if err != nil || len(info.Issues) != 0 {
		t.Fatalf("renewal fixture package: %v, %+v", err, info.Issues)
	}
	var record store.ConnectorRecord
	var declaration protocol.PluginConnectorDeclaration
	refresh, expired := "refresh-one", uint64(time.Now().Add(-time.Hour).UnixMilli())
	err = node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		if info, err = store.InstallPlugin(ctx, db, info, 0, true); err != nil {
			return err
		}
		capture, err := store.PrepareConnectorLogin(ctx, db, protocol.NewRequestID(), protocol.CommandStartConnectorLoginPayload{
			Package: plugins.PackageReference(info), Connector: "account", Method: protocol.PluginConnectorKindOauth2,
			Input: protocol.PluginConnectorInput{Fields: map[string]string{"client_id": "fixture-client"}, Secrets: map[string]protocol.Secret{}},
		})
		if err != nil {
			return err
		}
		if err := store.StartConnectorLogin(ctx, db, capture); err != nil {
			return err
		}
		if _, err := store.CompleteConnectorLogin(ctx, db, capture, &authorization.Issued{
			Access: "access-one", Refresh: &refresh, ExpiresAtMs: &expired, Extra: map[string]string{},
		}, nil, false); err != nil {
			return err
		}
		declaration = capture.Declaration
		record, err = store.ReadConnectorRecord(ctx, db, capture.Record.ID)
		return err
	})
	if err != nil {
		t.Fatal(err)
	}
	requests, endpoint, credential := 0, "https://fixture.test/token", "refresh_token=refresh-one"
	var answer accountHTTP
	node.auth.client = &http.Client{Transport: accountHTTP(func(request *http.Request) (*http.Response, error) {
		requests++
		if body, _ := io.ReadAll(request.Body); request.URL.String() != endpoint || !strings.Contains(string(body), credential) {
			t.Errorf("renewal request: %s %s", request.URL, body)
		}
		return answer(request)
	})}
	respond := func(status int, body string) accountHTTP {
		return func(*http.Request) (*http.Response, error) {
			return &http.Response{StatusCode: status, Header: http.Header{"Content-Type": {"application/json"}}, Body: io.NopCloser(strings.NewReader(body))}, nil
		}
	}
	stored := func(t *testing.T) store.ConnectorRecord {
		t.Helper()
		var current store.ConnectorRecord
		if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			current, err = store.ReadConnectorRecord(ctx, db, record.ID)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		return current
	}
	settled := func(t *testing.T, err error, code protocol.ErrorCode, marked bool) {
		t.Helper()
		var fault *protocol.Fault
		if code == "" && err != nil || code != "" && (!errors.As(err, &fault) || fault.Code != code) {
			t.Fatalf("renewal ended %v, want %q", err, code)
		}
		if current := stored(t); current.Grant == nil || (current.Grant.Extra["renewal_pending"] == "true") != marked {
			t.Fatalf("stored grant after the renewal: %+v, want the uncertain marker %v", current.Grant, marked)
		}
	}
	renew := func(t *testing.T, code protocol.ErrorCode, marked bool) *protocol.Secret {
		t.Helper()
		token, err := node.connectorAccess(ctx, record, declaration)
		settled(t, err, code, marked)
		return token
	}

	t.Run("an unreachable service can be tried again", func(t *testing.T) {
		answer = func(*http.Request) (*http.Response, error) {
			return nil, &net.OpError{Op: "dial", Net: "tcp", Err: errors.New("no route to host")}
		}
		renew(t, protocol.ErrorCodeUnavailable, false)
		renew(t, protocol.ErrorCodeUnavailable, false)
		if requests != 2 {
			t.Fatalf("an unreachable service was asked %d times in two uses", requests)
		}
	})
	t.Run("a refusal asks for a new sign-in without the uncertain marker", func(t *testing.T) {
		answer = respond(http.StatusBadRequest, `{"error":"invalid_grant"}`)
		renew(t, protocol.ErrorCodeExpired, false)
	})
	t.Run("a renewed grant replaces the expired one", func(t *testing.T) {
		answer = respond(http.StatusOK, `{"access_token":"access-two","refresh_token":"refresh-two","expires_in":3600}`)
		if token := renew(t, "", false); token == nil || token.Expose() != "access-two" {
			t.Fatalf("renewed token: %v", token)
		}
	})
	t.Run("an answer that settles nothing keeps the connector uncertain", func(t *testing.T) {
		// The grant is expired again, as the first three cases left it for
		// each other.
		if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
			current, err := store.ReadConnectorRecord(ctx, db, record.ID)
			if err != nil {
				return err
			}
			_, err = store.SaveConnectorGrant(ctx, db, current, authorization.Issued{Access: "access-two", Refresh: &refresh, ExpiresAtMs: &expired, Extra: map[string]string{}})
			return err
		}); err != nil {
			t.Fatal(err)
		}
		before := requests
		answer = func(*http.Request) (*http.Response, error) {
			return nil, &net.OpError{Op: "read", Net: "tcp", Err: errors.New("connection reset")}
		}
		renew(t, protocol.ErrorCodeOutcomeUnknown, true)
		renew(t, protocol.ErrorCodeOutcomeUnknown, true)
		if requests != before+1 {
			t.Fatalf("an uncertain connector was renewed again: %d requests", requests-before)
		}
	})
	t.Run("an MCP sign-in settles its renewals the same way", func(t *testing.T) {
		endpoint, credential = "https://fixture.test/mcp-token", "refresh_token=refresh-mcp"
		config := oauth2.Config{ClientID: "fixture-client", Endpoint: oauth2.Endpoint{TokenURL: endpoint, AuthStyle: oauth2.AuthStyleInParams}}
		for _, current := range []struct {
			name   string
			answer accountHTTP
			code   protocol.ErrorCode
			marked bool
		}{
			{"unreachable", func(*http.Request) (*http.Response, error) {
				return nil, &net.OpError{Op: "dial", Net: "tcp", Err: errors.New("no route to host")}
			}, protocol.ErrorCodeUnavailable, false},
			{"refused", respond(http.StatusBadRequest, `{"error":"invalid_grant"}`), protocol.ErrorCodeExpired, false},
			{"unsettled", respond(http.StatusServiceUnavailable, `{}`), protocol.ErrorCodeOutcomeUnknown, true},
			{"renewed", respond(http.StatusOK, `{"access_token":"access-mcp","token_type":"Bearer","refresh_token":"refresh-next","expires_in":3600}`), "", false},
		} {
			t.Run(current.name, func(t *testing.T) {
				var saved store.ConnectorRecord
				if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
					var err error
					saved, err = store.SaveConnectorGrant(ctx, db, stored(t), connectorOAuthIssued(&config, &oauth2.Token{
						AccessToken: "access-old", RefreshToken: "refresh-mcp", Expiry: time.Now().Add(-time.Hour),
					}))
					return err
				}); err != nil {
					t.Fatal(err)
				}
				source, err := node.connectorMCPSource(saved)
				if err != nil {
					t.Fatal(err)
				}
				answer = current.answer
				token, err := source.Token()
				settled(t, err, current.code, current.marked)
				if current.code == "" && token.AccessToken != "access-mcp" {
					t.Fatalf("renewed token: %+v", token)
				}
			})
		}
	})
}
