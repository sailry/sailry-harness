package telemetry

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"log/slog"
	"net/http"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/getsentry/sentry-go"
	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func setBuildProduct(t *testing.T, value string) {
	t.Helper()
	previous := BuildProduct
	BuildProduct = value
	t.Cleanup(func() { BuildProduct = previous })
}

func TestBuildProduct(t *testing.T) {
	for _, value := range []string{"", "harness", "sailry", "sailry-agent2", "a", strings.Repeat("a", 64),
		"Sailry", " sailry", "sailry ", "sailry\n", "2sailry", "-sailry", "sailry-", "sailry--agent",
		"sailry_agent", "sailry.agent", "sailry/agent", "sailry@1.0.0", "sailry\x00agent", "产品", strings.Repeat("a", 65)} {
		t.Run(value, func(t *testing.T) {
			setBuildProduct(t, value)
			want := "harness"
			switch value {
			case "harness", "sailry", "sailry-agent2", "a", strings.Repeat("a", 64):
				want = value
			}
			c := New(Options{Lookup: func(name string) string {
				if name == "SAILRY_PRODUCT" {
					return "runtime-product"
				}
				return ""
			}})
			defer c.Close()
			BuildProduct = "changed-after-new"
			if got := c.tagsLocked()["product"]; got != want {
				t.Fatalf("product = %q, want %q", got, want)
			}
		})
	}
}

func TestSourceProductIdentity(t *testing.T) {
	control := New(Options{Product: "code", Version: "0.1.0"})
	defer control.Close()
	if control.tagsLocked()["product"] != "code" || !strings.HasPrefix(control.release, "sailry@0.1.0+") {
		t.Fatalf("source product: %s %s", control.product, control.release)
	}
}

func TestProductCountersWireFormat(t *testing.T) {
	for _, product := range []string{"harness", "code", "dot", "ultra"} {
		for _, application := range []string{"desktop", "host"} {
			t.Run(product+"/"+application, func(t *testing.T) {
				setBuildProduct(t, product)
				remote := &endpoints{}
				c := New(Options{Application: application, Transport: remote, Timer: remote.timer, Packages: []string{"browser"}, Lookup: func(name string) string {
					if name == "SAILRY_TELEMETRY_KEY" {
						return "usage-key"
					}
					return ""
				}})
				defer c.Close()
				id := uuid.New()
				c.Configure(protocol.TelemetrySettings{InstallationID: id, Metrics: true})
				fault, status, retries, driver := protocol.ErrorCodePermissionDenied, uint16(429), uint64(2), protocol.SearchDriverExa
				tool, ok := BuiltinTool("read_session")
				if !ok {
					t.Fatal("built-in tool refused")
				}
				for range 2 {
					c.Observe(Turn("failed", protocol.WorkModeCode, true, &fault))
					c.Observe(ToolCall(tool, "denied", &fault, 3*time.Millisecond))
					feature, ok := c.FeatureUse(protocol.FeatureUseSearchPalette)
					if !ok {
						t.Fatal("listed feature refused")
					}
					c.Observe(feature)
				}
				c.Observe(ModelRequest(Model{API: protocol.ConversationModelApiResponses, Authentication: protocol.AuthenticationApiKey, Model: "custom"}, "error", &status, &fault, &retries, Tokens{Input: 1, Output: 2, CacheRead: 3, CacheWrite: 4}))
				c.Observe(Search(&driver, "error", &fault, time.Millisecond))
				c.Close()

				common := map[string]any{"product": product, "application": application, "installation_id": id.String(), "channel": c.channel, "release": c.release}
				for _, name := range []string{"os", "arch"} {
					common[name] = c.tagsLocked()[name]
					if common[name] == "" {
						t.Fatalf("missing %s tag", name)
					}
				}
				want := map[string]map[string]any{
					"app_launches":    {"count": float64(1)},
					"turns":           {"count": float64(2), "result": "failed", "mode": "code", "subagent": true, "error_code": "permission_denied"},
					"tool_calls":      {"count": float64(2), "tool": map[string]any{"source": "builtin", "name": "read_session"}, "result": "denied", "error_code": "permission_denied", "elapsed_us_total": float64(6000), "elapsed_us_max": float64(3000)},
					"model_requests":  {"count": float64(1), "api": "responses", "authentication": "api_key", "model": "custom", "result": "error", "http_status_class": "4xx", "error_code": "permission_denied", "retries": float64(2), "input": float64(1), "output": float64(2), "cache_read": float64(3), "cache_write": float64(4)},
					"search_requests": {"count": float64(1), "driver": "exa", "result": "error", "error_code": "permission_denied", "elapsed_us_total": float64(1000)},
					"feature_uses":    {"count": float64(2), "feature": string(protocol.FeatureUseSearchPalette)},
				}
				usage, reports := remote.sent()
				if len(usage) != len(want) || len(reports) != 0 {
					t.Fatalf("sent %d metric batches and %d reports", len(usage), len(reports))
				}
				for _, request := range remote.requests {
					if request.Method != http.MethodPost || request.URL.String() != metricsEndpoint || request.Header.Get("Authorization") != "Bearer usage-key" || request.Header.Get("Content-Type") != "application/json" {
						t.Errorf("invalid telemetry.sh request: %s %s %v", request.Method, request.URL, request.Header)
					}
				}
				for _, body := range usage {
					assertClean(t, "product counters", body)
					var envelope map[string]json.RawMessage
					if err := json.Unmarshal(body, &envelope); err != nil {
						t.Fatal(err)
					}
					if len(envelope) != 2 || envelope["table"] == nil || envelope["data"] == nil {
						t.Fatalf("invalid telemetry.sh envelope: %s", body)
					}
					var table string
					var rows []map[string]any
					if json.Unmarshal(envelope["table"], &table) != nil || json.Unmarshal(envelope["data"], &rows) != nil || len(rows) != 1 || want[table] == nil {
						t.Fatalf("invalid telemetry.sh batch: %s", body)
					}
					row := rows[0]
					if len(row) != len(common)+len(want[table])+3 {
						t.Errorf("%s row has missing or extra fields: %v", table, row)
					}
					for key, value := range common {
						if row[key] != value {
							t.Errorf("%s %s = %v, want %v", table, key, row[key], value)
						}
					}
					for key, value := range want[table] {
						if !reflect.DeepEqual(row[key], value) {
							t.Errorf("%s %s = %v, want %v", table, key, row[key], value)
						}
					}
					if event, ok := row["event_id"].(string); !ok {
						t.Error("missing event identity")
					} else if _, err := uuid.Parse(event); err != nil {
						t.Error(err)
					}
					for _, key := range []string{"window_start", "window_end"} {
						value, _ := row[key].(string)
						if _, err := time.Parse(time.RFC3339Nano, value); err != nil {
							t.Errorf("%s: %v", key, err)
						}
					}
					delete(want, table)
				}
				if len(want) != 0 {
					t.Errorf("missing counter tables: %v", want)
				}
			})
		}
	}
}

func productReportEvent(t *testing.T, body []byte) sentry.Event {
	t.Helper()
	lines := bytes.SplitN(body, []byte("\n"), 4)
	var event sentry.Event
	if len(lines) < 3 || json.Unmarshal(lines[2], &event) != nil {
		t.Fatalf("invalid Sentry envelope: %s", body)
	}
	return event
}

func TestProductSentryReports(t *testing.T) {
	for _, product := range []string{"harness", "code", "dot", "ultra"} {
		for _, application := range []string{"desktop", "host"} {
			t.Run(product+"/"+application, func(t *testing.T) {
				setBuildProduct(t, product)
				remote := &endpoints{}
				c := New(Options{Application: application, Transport: remote, Timer: remote.timer, Lookup: func(name string) string {
					if name == "SAILRY_SENTRY_DSN" {
						return retryDSN
					}
					return ""
				}})
				defer c.Close()
				id := uuid.New()
				c.Configure(protocol.TelemetrySettings{InstallationID: id, Errors: true})
				crumb := logging.Fields{"area": "runtime", "operation": "breadcrumb_only", "product": "runtime-product", "application": "runtime-app", "detail": plantedSecret}
				c.Record(slog.LevelWarn, crumb)
				fields := logging.Fields{"area": "command", "operation": "submit_turn", "phase": logging.Failed, "code": "unavailable", "reason_code": "draft_not_saveable", "count": uint64(3), "product": "runtime-product", "application": "runtime-app", "detail": plantedSecret}
				c.Record(slog.LevelError, fields)
				c.report("hang", reduce([]byte(crashTrace)))
				c.ReportCrash(&Crash{reduced: reduce([]byte(crashTrace))})
				c.reports.Wait()
				c.Configure(protocol.TelemetrySettings{InstallationID: id})
				if err := c.Diagnostics(context.Background()); err != nil {
					t.Fatal(err)
				}
				c.Close()
				usage, reports := remote.sent()
				if len(usage) != 0 || len(reports) != 4 {
					t.Fatalf("sent %d metric batches and %d reports", len(usage), len(reports))
				}
				crashArea := "interface"
				if application == "host" {
					crashArea = "host"
				}
				messages := map[string]bool{"command.submit_turn:unavailable": true, "interface.hang:unresponsive": true, crashArea + ".crash:panic.nil_dereference": true, "runtime.diagnostics:requested": true}
				for _, body := range reports {
					assertClean(t, "product report", body)
					event := productReportEvent(t, body)
					if !messages[event.Message] {
						t.Fatalf("unexpected or duplicate report %q", event.Message)
					}
					delete(messages, event.Message)
					for key, value := range map[string]string{"product": product, "application": application, "installation_id": id.String(), "channel": c.channel} {
						if event.Tags[key] != value {
							t.Errorf("%s tag %s = %q, want %q", event.Message, key, event.Tags[key], value)
						}
					}
					if event.Tags["os"] == "" || event.Tags["arch"] == "" || event.Release != c.release || event.Environment != c.channel || len(event.Fingerprint) != 1 {
						t.Errorf("existing report metadata changed: %+v", event)
					}
					tags := 6
					if event.Message == "command.submit_turn:unavailable" {
						tags++
						want := sentry.Context{"area": "command", "operation": "submit_turn", "phase": logging.Failed, "code": "unavailable", "reason_code": "draft_not_saveable", "count": float64(3)}
						if event.Tags["sailry.reason_code"] != "draft_not_saveable" || !reflect.DeepEqual(event.Contexts["sailry"], want) {
							t.Errorf("operation fields changed: %v, %v", event.Tags, event.Contexts)
						}
						if len(event.Breadcrumbs) != 1 || !reflect.DeepEqual(event.Breadcrumbs[0].Data, map[string]any{"area": "runtime", "operation": "breadcrumb_only"}) {
							t.Errorf("operation breadcrumbs changed: %v", event.Breadcrumbs)
						}
					}
					if len(event.Tags) != tags || bytes.Contains(body, []byte("runtime-product")) || bytes.Contains(body, []byte("runtime-app")) {
						t.Errorf("report identity polluted: %s", body)
					}
				}
			})
		}
	}
}

func TestProductOperationRetryRecovery(t *testing.T) {
	for _, test := range []struct {
		name, source, destination string
		replay                    bool
	}{
		{name: "legacy Harness with default product", source: "harness", replay: true},
		{name: "legacy Harness with explicit product", source: "harness", destination: "harness", replay: true},
		{name: "legacy Harness cannot become Sailry", source: "harness", destination: "sailry"},
		{name: "same Sailry product", source: "sailry", destination: "sailry", replay: true},
		{name: "Sailry cannot become Harness", source: "sailry", destination: "harness"},
		{name: "Sailry cannot become another product", source: "sailry", destination: "another-product"},
	} {
		t.Run(test.name, func(t *testing.T) {
			setBuildProduct(t, test.source)
			directory := t.TempDir()
			settings := protocol.TelemetrySettings{InstallationID: uuid.New(), Errors: true}
			original, first := operationControl(t, directory, retryDSN, false, settings, 503)
			recordFailure(original)
			initial := productReportEvent(t, receiveOperation(t, first.calls))
			receiveOperation(t, first.timers)
			name := filepath.Join(directory, operationName(0))
			body, err := os.ReadFile(name)
			if err != nil {
				t.Fatal(err)
			}
			var item pendingOperation
			if err := json.Unmarshal(body, &item); err != nil {
				t.Fatal(err)
			}
			legacy := sha256.Sum256([]byte(original.installation + "\x00" + original.dsn + "\x00" + original.application + "\x00" + original.release))
			if (item.Scope == hex.EncodeToString(legacy[:])) != (test.source == "harness") {
				t.Fatal("legacy Harness and product-specific scopes are not isolated")
			}
			if bytes.Contains(body, []byte(`"product"`)) || bytes.Contains(body, []byte(`"tags"`)) {
				t.Fatal("compact queue no longer matches the legacy untagged format")
			}
			original.Close()
			// Restore exactly what an interrupted process could leave at the same path.
			if err := os.WriteFile(name, body, 0o600); err != nil {
				t.Fatal(err)
			}
			setBuildProduct(t, test.destination)
			recovered, next := operationControl(t, directory, retryDSN, false, settings, 200)
			if recovered.dsn != original.dsn || recovered.application != original.application || recovered.release != original.release || recovered.installation != original.installation {
				t.Fatal("recovery changed a dimension other than product")
			}
			if test.replay {
				receiveOperation(t, next.timers)()
				report := receiveOperation(t, next.calls)
				assertClean(t, "product retry", report)
				event := productReportEvent(t, report)
				if event.Tags["product"] != productID(test.destination) || event.Tags["application"] != original.application || event.Tags["installation_id"] != settings.InstallationID.String() {
					t.Fatalf("replayed report identity = %v", event.Tags)
				}
				if event.EventID != initial.EventID || !event.Timestamp.Equal(initial.Timestamp) || !reflect.DeepEqual(event.Fingerprint, initial.Fingerprint) || !reflect.DeepEqual(event.Contexts, initial.Contexts) || event.Release != initial.Release {
					t.Fatal("replay changed existing reporting identities or public fields")
				}
			} else {
				for deadline := time.Now().Add(5 * time.Second); ; {
					if _, err := os.Lstat(name); os.IsNotExist(err) {
						break
					}
					if time.Now().After(deadline) {
						t.Fatal("another product's operation was not retired")
					}
					time.Sleep(time.Millisecond)
				}
				if len(next.timers) != 0 || len(next.calls) != 0 {
					t.Fatal("another product's operation was scheduled or replayed")
				}
			}
			recovered.Close()
		})
	}
}
