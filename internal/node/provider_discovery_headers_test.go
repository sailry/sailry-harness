package node

import (
	"context"
	"encoding/json"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestDiscoverModelsHeadersRemainPrivate(t *testing.T) {
	t.Setenv("SAILRY_TELEMETRY", "off")
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal("resolving the discovery fixture directory failed")
	}
	profile := filepath.Join(root, "profile")
	closeLog, err := logging.Install(profile, "host")
	if err != nil {
		t.Fatal("installing the discovery fixture log failed")
	}
	t.Cleanup(closeLog)
	node, err := StartWithOptions(t.Context(), profile, Options{MCP: fixtureMCPOptions()})
	if err != nil {
		t.Fatal("starting the discovery fixture Node failed")
	}
	t.Cleanup(func() { _ = node.Stop(context.Background()) })
	const marker = "private-discovery-fixture"
	var reached atomic.Bool
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		reached.Store(request.Header.Get("X-Tenant-Id") == marker)
		writer.WriteHeader(http.StatusServiceUnavailable)
		_, _ = writer.Write([]byte(marker))
	}))
	defer server.Close()
	secret := protocol.NewSecret("fixture-key")
	request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "discover_models", Data: protocol.ConversationDiscoverySource{Kind: "draft", Data: protocol.ConversationDiscoveryDraft{
		Provider: protocol.NewProviderID(), Api: protocol.ConversationModelApiResponses, Endpoint: server.URL, Secret: &secret, Headers: map[string]string{"X-Tenant-Id": marker},
	}}})
	recorded := make(chan logging.Fields, 1)
	logging.SetReport(func(_ slog.Level, fields logging.Fields) {
		if fields["request"] == request.ID.String() {
			select {
			case recorded <- fields:
			default:
			}
		}
	})
	t.Cleanup(func() { logging.SetReport(nil) })
	fault := dispatchFixture(t, node, node.ID(), request).Fault
	if !reached.Load() || fault == nil || fault.Code != protocol.ErrorCodeUnavailable || !logging.ValidReportRef(fault.ReportRef) {
		t.Fatal("the gateway failure did not retain its safe discovery outcome")
	}
	encoded, err := json.Marshal(fault)
	if err != nil || strings.Contains(string(encoded), marker) {
		t.Fatal("the discovery fault disclosed a private header value")
	}
	select {
	case local := <-recorded:
		for _, fields := range []logging.Fields{local, logging.Remote(local)} {
			encoded, err := json.Marshal(fields)
			if err != nil || strings.Contains(string(encoded), marker) {
				t.Fatal("discovery diagnostics disclosed a private header value")
			}
		}
	default:
		t.Fatal("the discovery failure was not recorded")
	}
	entries, err := os.ReadDir(logging.Directory())
	if err != nil {
		t.Fatal("reading the discovery fixture logs failed")
	}
	for _, entry := range entries {
		if entry.IsDir() {
			continue
		}
		data, err := os.ReadFile(filepath.Join(logging.Directory(), entry.Name()))
		if err != nil || strings.Contains(string(data), marker) {
			t.Fatal("the local discovery log disclosed a private header value")
		}
	}
}
