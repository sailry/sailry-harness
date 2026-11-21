package main

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"sync"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestAcceptanceCacheToolsListAndIsolation(t *testing.T) {
	profile := filepath.Join(t.TempDir(), "unopened")
	var output bytes.Buffer
	code, err := runAcceptance(t.Context(), []string{"--list", "--suite", cacheToolsSuite, "--data-dir", profile}, &output)
	events := decodeAcceptanceEvents(t, output.Bytes())
	if err != nil || code != 0 || len(events) != 1 || events[0].Case == nil || events[0].Case.ID != cacheToolsSuite {
		t.Fatalf("list exit %d, error %v, events %s", code, err, output.Bytes())
	}
	for _, args := range [][]string{
		{"--suite", cacheToolsSuite, "--data-dir", profile},
		{"--suite", cacheToolsSuite, "--data-dir", profile, "--start", "--case", "A01"},
		{"--suite", "backend", "--data-dir", profile, "--start", "--case", cacheToolsSuite},
	} {
		if code, err := runAcceptance(t.Context(), args, &output); code != 1 || err == nil {
			t.Fatalf("accepted invalid cache-tools invocation %v", args)
		}
	}
	if _, err := os.Stat(profile); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("list or invalid invocation opened its profile: %v", err)
	}
}

func TestAcceptanceCacheToolsResponsesFixture(t *testing.T) {
	const (
		modelID    = "gpt-6.2"
		alias      = "gpt-6.2-fixture-snapshot"
		fixtureKey = "private-cache-tools-fixture-key"
		fixtureOut = "private-cache-tools-fixture-output"
	)
	var mu sync.Mutex
	var requests []map[string]any
	var scenario string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodPost || r.URL.Path != "/v1/responses" || r.Header.Get("Authorization") != "Bearer "+fixtureKey || r.Header.Get("X-Cache-Tools-Fixture") != "fixture" {
			http.Error(w, fixtureOut, http.StatusBadRequest)
			t.Error("request did not use the configured Responses endpoint and credential")
			return
		}
		var body map[string]any
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			http.Error(w, fixtureOut, http.StatusBadRequest)
			t.Error("request had an invalid JSON body")
			return
		}
		if key, _ := body["prompt_cache_key"].(string); key == "" || key != r.Header.Get("X-Session-Affinity") || key != r.Header.Get("X-Session-Id") {
			t.Error("group cache key and normal transport affinity did not match")
		}
		mu.Lock()
		index, currentScenario := len(requests), scenario
		requests = append(requests, body)
		mu.Unlock()
		if currentScenario == "provider error" && index == 2 {
			http.Error(w, fixtureOut, http.StatusInternalServerError)
			return
		}
		input, cached := 3200, 0
		if index%2 == 1 {
			input += 250
		}
		if index >= 2 {
			cached = 3072
			if index%2 == 1 {
				cached += 128
			}
		}
		usage := map[string]any{"input_tokens": input, "input_tokens_details": map[string]any{"cached_tokens": cached}, "output_tokens": 1, "total_tokens": input + 1}
		if currentScenario == "missing usage" && index == 2 {
			delete(usage, "input_tokens_details")
		}
		if currentScenario == "missing usage" && index == 3 {
			usage = nil
		}
		if currentScenario == "no warm hit" && index == 3 {
			usage["input_tokens_details"] = map[string]any{"cached_tokens": 0}
		}
		output := []any{map[string]any{"type": "message", "id": "message", "role": "assistant", "status": "completed",
			"content": []any{map[string]any{"type": "output_text", "text": fixtureOut, "annotations": []any{}}}}}
		response := map[string]any{"id": "response", "object": "response", "created_at": 1, "status": "completed", "model": alias,
			"output": output, "usage": usage}
		event := "response.completed"
		if index == 5 {
			response["output"] = append(output, map[string]any{"type": "function_call", "id": "call-item", "call_id": "call-fixture", "name": "read_file", "arguments": `{"path":"private-fixture-path"}`, "status": "completed"})
			response["status"] = "incomplete"
			response["incomplete_details"] = map[string]any{"reason": "max_output_tokens"}
			event = "response.incomplete"
		}
		if currentScenario == "different model" && index == 3 {
			response["model"] = "private-alternate-model"
		}
		data, _ := json.Marshal(map[string]any{"type": event, "sequence_number": 0, "response": response})
		w.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprintf(w, "event: %s\ndata: %s\n\n", event, data)
	}))
	t.Cleanup(server.Close)

	profile := filepath.Join(t.TempDir(), "profile")
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Enabled, provider.Name = protocol.NewProviderID(), true, "Cache fixture"
	provider.Api, provider.Authentication, provider.Endpoint = protocol.ConversationModelApiResponses, protocol.AuthenticationApiKey, server.URL+"/v1"
	provider.Headers = map[string]string{"X-Cache-Tools-Fixture": "fixture"}
	provider.DefaultModel = modelID
	listed := protocol.DefaultConversationModel()
	listed.ID, listed.Context, listed.Output, listed.Tools, listed.Reasoning = modelID, 32000, 4096, true, true
	listed.DefaultEffort = protocol.Effort{Kind: "low"}
	listed.Efforts = []protocol.Effort{listed.DefaultEffort, {Kind: "medium"}}
	provider.Models = []protocol.ConversationModel{listed}
	secret := protocol.NewSecret(fixtureKey)
	controller, _, closeNode, err := acceptanceClient(t.Context(), acceptanceOptions{profile: profile, start: true})
	if err != nil {
		t.Fatal(err)
	}
	_, saveErr := controller.Do(t.Context(), protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{Provider: provider, Secret: &secret}})
	if err := errors.Join(saveErr, closeNode()); err != nil {
		t.Fatal(err)
	}

	seenIDs := make(map[string]bool)
	for _, tc := range []struct {
		name         string
		requestCount int
		status       string
		detail       string
	}{
		{"paired hot samples", 6, "passed", "+122 and +122"},
		{"missing usage", 6, "blocked", "did not report both input and cached token counts"},
		{"different model", 6, "blocked", "reported different models"},
		{"no warm hit", 6, "blocked", "reported no cache hit"},
		{"provider error", 3, "blocked", "The Responses request failed"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			mu.Lock()
			scenario, requests = tc.name, nil
			mu.Unlock()
			var output bytes.Buffer
			code, err := runAcceptance(t.Context(), []string{"--suite", cacheToolsSuite, "--data-dir", profile, "--start", "--provider", provider.Name, "--model", modelID, "--effort", "medium"}, &output)
			wantCode := 1
			if tc.status == "passed" {
				wantCode = 0
			}
			if err != nil || code != wantCode {
				t.Fatalf("acceptance exit %d, want %d, error %v, output %s", code, wantCode, err, output.Bytes())
			}
			events := decodeAcceptanceEvents(t, output.Bytes())
			last := events[len(events)-1]
			if last.Report == nil || last.Files == nil || len(last.Report.Cases) != 1 || last.Report.Provider != provider.ID.String() || last.Report.Model != modelID || last.Report.Effort == nil || last.Report.Effort.Kind != "medium" {
				t.Fatalf("missing selected provider, model, effort or report: %+v", last)
			}
			if last.Files.JSON != filepath.Join(profile, "acceptance", cacheToolsSuite, "latest.json") {
				t.Fatalf("unexpected report file %q", last.Files.JSON)
			}
			result := last.Report.Cases[0]
			if result.Status != tc.status || !strings.Contains(result.Detail, tc.detail) || len(result.Evidence) != tc.requestCount || len(result.OwnedSessions) != 0 {
				t.Fatalf("unexpected result: %+v", result)
			}
			for _, file := range []string{last.Files.JSON, last.Files.HTML} {
				contents, err := os.ReadFile(file)
				if err != nil {
					t.Fatal(err)
				}
				for _, private := range []string{server.URL, fixtureKey, fixtureOut, alias, "private-alternate-model", "private-fixture-path", "Synthetic group"} {
					if bytes.Contains(contents, []byte(private)) || bytes.Contains(output.Bytes(), []byte(private)) {
						t.Fatal("acceptance printed private request or response data")
					}
				}
			}
			mu.Lock()
			captured := requests
			mu.Unlock()
			if len(captured) != tc.requestCount {
				t.Fatalf("got %d HTTP requests, want %d", len(captured), tc.requestCount)
			}
			for i, evidence := range result.Evidence {
				var sample model.CacheToolsSample
				if evidence.Kind != "cache-tools.request" || json.Unmarshal([]byte(evidence.Value), &sample) != nil {
					t.Fatalf("invalid per-request evidence %+v", evidence)
				}
				group, phase := "without_tools", "cold"
				input, cached := uint64(3200), uint64(0)
				if i%2 == 1 {
					group = "with_tools"
					input += 250
				}
				if i >= 2 {
					phase = "warm"
					cached = 3072 + uint64(i%2)*128
				}
				if tc.name == "no warm hit" && i == 3 {
					cached = 0
				}
				want := model.CacheToolsSample{Group: group, Phase: phase, Index: i/2 + 1, Input: &input, Cached: &cached, Fresh: new(input - cached), ResponseModel: "consistent"}
				switch {
				case tc.name == "missing usage" && i == 2:
					want.Cached, want.Fresh = nil, nil
				case tc.name == "missing usage" && i == 3:
					want.Input, want.Cached, want.Fresh = nil, nil, nil
				case tc.name == "provider error" && i == 2:
					want.Input, want.Cached, want.Fresh, want.ResponseModel = nil, nil, nil, "missing"
				case tc.name == "different model" && i == 3:
					want.ResponseModel = "different"
				}
				if !reflect.DeepEqual(sample, want) {
					t.Fatalf("sample %d: %s does not match expected %#v", i+1, evidence.Value, want)
				}
			}
			checkCacheToolsWire(t, captured, modelID, seenIDs)
		})
	}
}

func checkCacheToolsWire(t *testing.T, captured []map[string]any, modelID string, seenIDs map[string]bool) {
	t.Helper()
	var keys [2]string
	var runID, sharedReference string
	var shared map[string]any
	var toolDeclarations []any
	for i, body := range captured {
		key, _ := body["prompt_cache_key"].(string)
		tranche := i % 2
		if i < 2 {
			keys[tranche] = key
			if key == "" || seenIDs[key] {
				t.Fatal("cache affinity was reused between groups or runs")
			}
			seenIDs[key] = true
		}
		if key != keys[tranche] {
			t.Fatal("cache affinity changed within a group")
		}
		if body["model"] != modelID || body["max_output_tokens"] != float64(64) || body["store"] != false || body["stream"] != true {
			t.Fatal("request lost the configured model or cost bounds")
		}
		reasoning, _ := body["reasoning"].(map[string]any)
		if reasoning["effort"] != "medium" {
			t.Fatal("request lost the selected reasoning effort")
		}
		tools, _ := body["tools"].([]any)
		if tranche == 0 && len(tools) != 0 || tranche == 1 && len(tools) != 3 {
			t.Fatalf("got %d tool declarations for group %d", len(tools), tranche)
		}
		if i == 1 {
			toolDeclarations = tools
			for j, name := range []string{"list_directory", "read_file", "search_files"} {
				tool, _ := tools[j].(map[string]any)
				parameters, _ := tool["parameters"].(map[string]any)
				properties, _ := parameters["properties"].(map[string]any)
				if tool["name"] != name || tool["type"] != "function" || tool["description"] == "" || parameters["type"] != "object" || len(properties) == 0 {
					t.Fatalf("tool declaration %d has no representative schema", j)
				}
			}
		} else if tranche == 1 && !reflect.DeepEqual(tools, toolDeclarations) {
			t.Fatal("tool declarations changed within the tools group")
		}
		input, _ := body["input"].([]any)
		if len(input) != 3 {
			t.Fatalf("expected developer, synthetic reference and sample messages, got %d items", len(input))
		}
		userText := func(item any) map[string]any {
			message, _ := item.(map[string]any)
			content, _ := message["content"].([]any)
			if message["role"] != "user" || len(content) != 1 {
				t.Fatal("lost synthetic user input")
			}
			part, _ := content[0].(map[string]any)
			return part
		}
		prefix, suffix := userText(input[1]), userText(input[2])
		text, _ := prefix["text"].(string)
		firstLine, reference, hasReference := strings.Cut(text, "\n")
		namespace, labeled := strings.CutPrefix(firstLine, fmt.Sprintf("Synthetic group %c for run ", 'A'+tranche))
		if _, err := protocol.ParseSessionID(namespace); err != nil || !labeled || !hasReference || len(reference) < 12000 || suffix["text"] != fmt.Sprintf("Sample marker %d. Reply with OK only.", i/2+1) {
			t.Fatal("request did not use the synthetic reference and paired sample marker")
		}
		if i == 0 {
			if seenIDs[namespace] {
				t.Fatal("reused a synthetic run namespace")
			}
			seenIDs[namespace] = true
			runID, sharedReference = namespace, reference
		} else if namespace != runID || reference != sharedReference {
			t.Fatal("base prompt changed across groups or samples")
		}
		prefix["text"], suffix["text"] = reference, "fixture sample marker"
		delete(body, "tools")
		delete(body, "prompt_cache_key")
		if i == 0 {
			shared = body
		} else if !reflect.DeepEqual(body, shared) {
			t.Fatal("request parameters differed outside the tool declarations, nonce and suffix")
		}
	}
}
