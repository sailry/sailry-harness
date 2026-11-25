package store

import (
	"database/sql"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestDraftDiscoveryForwardsHeaders(t *testing.T) {
	t.Parallel()
	for _, api := range []protocol.ConversationModelApi{protocol.ConversationModelApiResponses, protocol.ConversationModelApiAnthropic} {
		t.Run(string(api), func(t *testing.T) {
			const marker = "private-discovery-fixture"
			var requests atomic.Int32
			server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
				requests.Add(1)
				credential := request.Header.Get("Authorization") == "Bearer fixture-key"
				if api == protocol.ConversationModelApiAnthropic {
					credential = request.Header.Get("x-api-key") == "fixture-key"
				}
				if request.Header.Get("X-Tenant-Id") != marker || request.Header.Get("User-Agent") != "fixture-client" || !credential {
					writer.WriteHeader(http.StatusServiceUnavailable)
					return
				}
				writer.Header().Set("Content-Type", "application/json")
				_, _ = writer.Write([]byte(`{"data":[{"id":"fixture-model"}],"has_more":false}`))
			}))
			defer server.Close()
			_, node, storage := schemaFixture(t)
			secret := protocol.NewSecret("fixture-key")
			draft := protocol.ConversationDiscoveryDraft{Provider: protocol.NewProviderID(), Api: api, Endpoint: server.URL, Secret: &secret,
				Headers: map[string]string{"X-Tenant-Id": marker, "User-Agent": "fixture-client"}}
			encoded, err := json.Marshal(protocol.NewRequest(node, protocol.Command{Kind: "discover_models", Data: protocol.ConversationDiscoverySource{Kind: "draft", Data: draft}}))
			if err != nil {
				t.Fatal("encoding the draft discovery request failed")
			}
			var request protocol.Request
			if protocol.DecodeAs(protocol.PeerData, encoded, &request) != nil {
				t.Fatal("decoding the draft discovery request failed")
			}
			var work *providers.Work
			if storage.Node.Read(t.Context(), func(db *sql.Conn) error {
				var err error
				work, err = PrepareProviderCommand(t.Context(), db, node, request)
				return err
			}) != nil || work == nil || work.Provider == nil || work.Provider.Headers["X-Tenant-Id"] != marker {
				t.Fatal("preparing discovery lost the custom request headers")
			}
			work.Client = server.Client()
			prepared, err := providers.Run(t.Context(), *work)
			if err != nil || prepared == nil || prepared.Output == nil || requests.Load() != 1 {
				t.Fatal("draft discovery failed to reach the gateway with its configured headers")
			}
			if storage.Node.Write(t.Context(), func(db *sql.Conn) error {
				output, _, err := ExecuteProviderCommand(t.Context(), db, node, request, prepared)
				if err == nil && output.Kind != "discovered_models" {
					t.Error("draft discovery returned an unexpected output")
				}
				return err
			}) != nil {
				t.Fatal("completing draft discovery failed")
			}
		})
	}
}

func TestDraftDiscoveryValidatesHeadersBeforeNetwork(t *testing.T) {
	t.Parallel()
	const marker = "private-discovery-fixture"
	tooMany := map[string]string{}
	for index := range 33 {
		tooMany["X-"+strconv.Itoa(index)] = marker
	}
	for name, headers := range map[string]map[string]string{
		"credential":     {"Authorization": marker},
		"api key":        {"X-Api-Key": marker},
		"framing":        {"Content-Length": marker},
		"invalid name":   {"bad name": marker},
		"invalid value":  {"X-Tenant-Id": marker + "\r\nInjected: value"},
		"case duplicate": {"X-Tenant-Id": marker, "x-tenant-id": marker},
		"oversized":      {"X-Tenant-Id": strings.Repeat(marker, 256)},
		"too many":       tooMany,
	} {
		t.Run(name, func(t *testing.T) {
			_, node, storage := schemaFixture(t)
			request := protocol.NewRequest(node, protocol.Command{Kind: "discover_models", Data: protocol.ConversationDiscoverySource{Kind: "draft", Data: protocol.ConversationDiscoveryDraft{
				Provider: protocol.NewProviderID(), Api: protocol.ConversationModelApiResponses, Endpoint: "https://gateway.example/v1", Headers: headers,
			}}})
			var work *providers.Work
			err := storage.Node.Read(t.Context(), func(db *sql.Conn) error {
				var err error
				work, err = PrepareProviderCommand(t.Context(), db, node, request)
				return err
			})
			if !providerFixtureFault(err, protocol.ErrorCodeInvalidRequest) || work != nil {
				t.Fatal("discovery accepted invalid headers for network work")
			}
			if strings.Contains(err.Error(), marker) {
				t.Fatal("header validation disclosed a private header value")
			}
		})
	}
}
