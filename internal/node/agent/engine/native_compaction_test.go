package engine

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/prompt"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// wireRequest is one request the fake provider received.
type wireRequest struct {
	path   string
	header http.Header
	body   map[string]any
	raw    string
}

type fakeProvider struct {
	mu       sync.Mutex
	requests []wireRequest
}

func (p *fakeProvider) all(match func(wireRequest) bool) []wireRequest {
	p.mu.Lock()
	defer p.mu.Unlock()
	var out []wireRequest
	for _, request := range p.requests {
		if match(request) {
			out = append(out, request)
		}
	}
	return out
}

func compacting(request wireRequest) bool {
	return request.body["compaction"] != nil || strings.HasSuffix(request.path, "/responses/compact")
}

// nativeFixture points the fixture's provider at a fake server for api. The
// provider lists a second model so a switch can be exercised.
func nativeFixture(t *testing.T, api protocol.ConversationModelApi, handler func(http.ResponseWriter, wireRequest)) (*engineFixture, *execution, model.Binding, *fakeProvider) {
	t.Helper()
	provider := &fakeProvider{}
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		raw, _ := io.ReadAll(request.Body)
		value := wireRequest{path: request.URL.Path, header: request.Header.Clone(), raw: string(raw)}
		_ = json.Unmarshal(raw, &value.body)
		provider.mu.Lock()
		provider.requests = append(provider.requests, value)
		provider.mu.Unlock()
		handler(writer, value)
	}))
	t.Cleanup(server.Close)
	f, x, binding, _ := compactionFixture(t, &scriptedModel{})
	binding.Provider.Api = api
	binding.Provider.Endpoint = server.URL + "/v1"
	other := binding.Provider.Models[0]
	other.ID = "other"
	binding.Provider.Models = append(binding.Provider.Models, other)
	f.r.model = model.New
	// A manual compaction without a focus of its own.
	x.inv.Message.Text = ""
	return f, x, binding, provider
}

func sse(events ...string) string {
	var out strings.Builder
	for _, event := range events {
		var head struct {
			Type string `json:"type"`
		}
		_ = json.Unmarshal([]byte(event), &head)
		fmt.Fprintf(&out, "event: %s\ndata: %s\n\n", head.Type, event)
	}
	return out.String()
}

func writeSSE(writer http.ResponseWriter, body string) {
	writer.Header().Set("Content-Type", "text/event-stream")
	_, _ = io.WriteString(writer, body)
}

func writeJSON(writer http.ResponseWriter, status int, body string) {
	writer.Header().Set("Content-Type", "application/json")
	writer.WriteHeader(status)
	_, _ = io.WriteString(writer, body)
}

func anthropicText(text string) string {
	encoded, _ := json.Marshal(text)
	return sse(`{"type":"message_start","message":{"id":"msg","type":"message","role":"assistant","model":"fixture","content":[],"usage":{"input_tokens":10,"output_tokens":0}}}`,
		`{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}`,
		`{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":`+string(encoded)+`}}`,
		`{"type":"content_block_stop","index":0}`,
		`{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":5}}`,
		`{"type":"message_stop"}`)
}

// anthropicCompaction is an on-demand summary: one whole compaction block,
// billed in usage.iterations. A null content is a blocked summary.
func anthropicCompaction(content, stop string) string {
	return sse(`{"type":"message_start","message":{"id":"msg","type":"message","role":"assistant","model":"fixture","content":[],"usage":{"input_tokens":0,"output_tokens":0}}}`,
		`{"type":"content_block_start","index":0,"content_block":{"type":"compaction","content":`+content+`,"encrypted_content":"enc-1","signature":"sig-1","tool_changes":[{"type":"tool_addition","tool":{"name":"read_file"}}]}}`,
		`{"type":"content_block_stop","index":0}`,
		`{"type":"message_delta","delta":{"stop_reason":"`+stop+`"},"usage":{"input_tokens":0,"output_tokens":0,"iterations":[{"type":"compaction","input_tokens":144,"output_tokens":276}]}}`,
		`{"type":"message_stop"}`)
}

func responsesText(text string) string {
	encoded, _ := json.Marshal(text)
	return sse(`{"type":"response.created","sequence_number":0,"response":{"id":"r","object":"response","created_at":1,"model":"fixture","status":"in_progress","output":[]}}`,
		`{"type":"response.completed","sequence_number":1,"response":{"id":"r","object":"response","created_at":1,"model":"fixture","status":"completed","output":[{"id":"m","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","annotations":[],"text":`+string(encoded)+`}]}],"usage":{"input_tokens":12,"output_tokens":3,"total_tokens":15}}}`)
}

const compactedWindow = `{"id":"cmp","object":"response.compaction","created_at":1,"output":[{"type":"message","role":"user","status":"completed","content":[{"type":"input_text","text":"Original task"}]},{"type":"message","role":"user","content":"Plain <note>"},{"type":"compaction","id":"cmp_item","encrypted_content":"opaque-window"}],"usage":{"input_tokens":100,"output_tokens":20,"total_tokens":120}}`

func latestCheckpoint(t *testing.T, f *engineFixture) (canonical.Record, *canonical.Compaction) {
	t.Helper()
	records := f.state().Records
	for i := len(records) - 1; i >= 0; i-- {
		if records[i].Kind == "compaction" {
			return records[i], records[i].Data.(*canonical.Compaction)
		}
	}
	t.Fatal("no checkpoint was saved")
	return canonical.Record{}, nil
}

// addExchange appends a completed user request and reply.
func addExchange(t *testing.T, f *engineFixture, question string) {
	t.Helper()
	turn := f.state().Records[len(f.state().Records)-1].Turn
	user := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, question)), f.r.clock.Now())
	user.Metadata.Runtime = &canonical.Runtime{UserVisible: true, AgentVisible: true}
	reply := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Answer to "+question)), f.r.clock.Now())
	reply.Metadata.Runtime = &canonical.Runtime{UserVisible: true, AgentVisible: true}
	reply.Provenance.Author = "assistant"
	if err := f.r.save(f.ctx, f.session.ID, user, reply); err != nil {
		t.Fatal(err)
	}
}

// compactNow runs a manual compaction over the current history on binding's route.
func compactNow(t *testing.T, f *engineFixture, x *execution, binding model.Binding) error {
	t.Helper()
	languageModel, err := model.New(f.ctx, binding)
	if err != nil {
		t.Fatal(err)
	}
	return f.r.compact(x, binding, model.RouteOf(languageModel, f.session.ID), f.state(), true)
}

// sendMain sends the next main request for binding and returns its body.
func sendMain(t *testing.T, f *engineFixture, x *execution, binding model.Binding, provider *fakeProvider) wireRequest {
	t.Helper()
	request, err := f.r.mainRequest(x, binding, f.state(), prompt.Instruction(false))
	if err != nil {
		t.Fatal(err)
	}
	call, err := model.BuildVisibleProjectedCall(binding, request)
	if err != nil {
		t.Fatal(err)
	}
	languageModel, err := model.New(f.ctx, binding)
	if err != nil {
		t.Fatal(err)
	}
	before := len(provider.all(func(wireRequest) bool { return true }))
	if err := model.Run(f.ctx, languageModel, call, model.Main, f.r.clock, nil, nil, nil); err != nil {
		t.Fatal(err)
	}
	sent := provider.all(func(wireRequest) bool { return true })
	if len(sent) != before+1 {
		t.Fatalf("main request sent %d requests", len(sent)-before)
	}
	return sent[len(sent)-1]
}

func withModel(binding model.Binding, id string) model.Binding {
	binding.Config.Model = id
	return binding
}

func TestAnthropicNativeCompaction(t *testing.T) {
	t.Run("the signed block replaces the history and reaches only its model", func(t *testing.T) {
		f, x, binding, provider := nativeFixture(t, protocol.ConversationModelApiAnthropic, func(writer http.ResponseWriter, request wireRequest) {
			if compacting(request) {
				writeSSE(writer, anthropicCompaction(`"Readable summary of the task"`, "compaction"))
				return
			}
			writeSSE(writer, anthropicText("Done"))
		})
		if err := compactNow(t, f, x, binding); err != nil {
			t.Fatal(err)
		}
		sent := provider.all(compacting)
		if len(sent) != 1 || len(provider.all(func(wireRequest) bool { return true })) != 1 {
			t.Fatal("native compaction did not make exactly one request")
		}
		ask := sent[0]
		if fmt.Sprint(ask.body["compaction"]) != "map[type:summarize]" || ask.body["context_management"] != nil || !strings.Contains(ask.header.Get("anthropic-beta"), "compact-2026-09-04") || !strings.Contains(ask.raw, "Original task") {
			t.Fatalf("compaction request = %s, beta %q", ask.raw, ask.header.Get("anthropic-beta"))
		}
		record, checkpoint := latestCheckpoint(t, f)
		if !checkpoint.Readable() || checkpoint.Summary.Content[0].Data.(*canonical.TextContent).Text != "Readable summary of the task" {
			t.Fatal("the readable summary was not stored as the handoff")
		}
		if checkpoint.Native == nil || checkpoint.Native.Source.ID != binding.Provider.ID || checkpoint.Native.Source.Model != "fixture" || !strings.Contains(string(checkpoint.Native.Items), `"signature":"sig-1"`) {
			t.Fatalf("native checkpoint = %+v", checkpoint.Native)
		}
		usage := checkpoint.Summary.Usage
		if usage == nil || usage.Input == nil || *usage.Input != 144 || usage.Output == nil || *usage.Output != 276 {
			t.Fatalf("compaction usage = %+v, want the billed iteration", usage)
		}
		if execution := record.Metadata.Execution; execution == nil || execution.Purpose == nil || *execution.Purpose != canonical.PurposeCompaction || execution.Observation == nil {
			t.Fatal("the native compaction call was not observed as a compaction request")
		}

		same := sendMain(t, f, x, binding, provider)
		messages := same.body["messages"].([]any)
		first := messages[0].(map[string]any)
		block := first["content"].([]any)[0].(map[string]any)
		if first["role"] != "assistant" || block["type"] != "compaction" || block["signature"] != "sig-1" || block["content"] != "Readable summary of the task" || block["encrypted_content"] != "enc-1" || fmt.Sprint(block["tool_changes"]) != "[map[tool:map[name:read_file] type:tool_addition]]" || len(block) != 5 {
			t.Fatalf("same model history opens with %v", first)
		}
		if strings.Contains(same.raw, "Original task") || !strings.Contains(same.header.Get("anthropic-beta"), "compact-2026-09-04") {
			t.Fatal("the summarized messages or the beta flag were not swapped for the block")
		}
		other := sendMain(t, f, x, withModel(binding, "other"), provider)
		if strings.Contains(other.raw, `"compaction"`) || strings.Contains(other.raw, "sig-1") || !strings.Contains(other.raw, "Readable summary of the task") || strings.Contains(other.header.Get("anthropic-beta"), "compact-2026-09-04") {
			t.Fatalf("another model did not read the text summary: %s", other.raw)
		}
	})
	t.Run("a refused summary falls back to the handoff", func(t *testing.T) {
		f, x, binding, provider := nativeFixture(t, protocol.ConversationModelApiAnthropic, func(writer http.ResponseWriter, request wireRequest) {
			if compacting(request) {
				writeSSE(writer, anthropicCompaction("null", "refusal"))
				return
			}
			writeSSE(writer, anthropicText(inPlaceHandoff))
		})
		if err := compactNow(t, f, x, binding); err != nil {
			t.Fatal(err)
		}
		_, checkpoint := latestCheckpoint(t, f)
		if checkpoint.Native != nil || !checkpoint.Readable() || !strings.Contains(checkpoint.Summary.Content[0].Data.(*canonical.TextContent).Text, "Continue the task") {
			t.Fatal("a refused native summary did not fall back to Sailry's handoff")
		}
		addExchange(t, f, "Second request")
		if err := compactNow(t, f, x, binding); err != nil {
			t.Fatal(err)
		}
		if len(provider.all(compacting)) != 2 {
			t.Fatal("a refusal marked the route as unsupported")
		}
	})
	t.Run("an ordinary reply without a block falls back and is remembered", func(t *testing.T) {
		// A gateway that drops the beta, or a model without compaction,
		// answers the request as a normal turn.
		f, x, binding, provider := nativeFixture(t, protocol.ConversationModelApiAnthropic, func(writer http.ResponseWriter, request wireRequest) {
			writeSSE(writer, anthropicText(inPlaceHandoff))
		})
		if err := compactNow(t, f, x, binding); err != nil {
			t.Fatal(err)
		}
		if _, checkpoint := latestCheckpoint(t, f); checkpoint.Native != nil || !checkpoint.Readable() {
			t.Fatal("a reply without a compaction block did not fall back to Sailry's handoff")
		}
		addExchange(t, f, "Second request")
		if err := compactNow(t, f, x, binding); err != nil {
			t.Fatal(err)
		}
		if len(provider.all(compacting)) != 1 {
			t.Fatal("a route that ignored native compaction was asked again")
		}
	})
	t.Run("a rejected beta falls back and is remembered", func(t *testing.T) {
		f, x, binding, provider := nativeFixture(t, protocol.ConversationModelApiAnthropic, func(writer http.ResponseWriter, request wireRequest) {
			if compacting(request) {
				writeJSON(writer, http.StatusBadRequest, `{"type":"error","error":{"type":"invalid_request_error","message":"Unexpected value(s) `+"`compact-2026-09-04`"+` for the `+"`anthropic-beta`"+` header."}}`)
				return
			}
			writeSSE(writer, anthropicText(inPlaceHandoff))
		})
		if err := compactNow(t, f, x, binding); err != nil {
			t.Fatal(err)
		}
		if _, checkpoint := latestCheckpoint(t, f); checkpoint.Native != nil || !checkpoint.Readable() {
			t.Fatal("a rejected beta did not fall back to Sailry's handoff")
		}
		addExchange(t, f, "Second request")
		if err := compactNow(t, f, x, binding); err != nil {
			t.Fatal(err)
		}
		if len(provider.all(compacting)) != 1 {
			t.Fatal("a route that rejected native compaction was asked again")
		}
	})
	t.Run("a persistent gateway error falls back after one resend", func(t *testing.T) {
		// A relay that forwards /responses/compact to an upstream without it
		// answers 502 every time.
		f, x, binding, provider := nativeFixture(t, protocol.ConversationModelApiAnthropic, func(writer http.ResponseWriter, request wireRequest) {
			if compacting(request) {
				writeJSON(writer, http.StatusBadGateway, `{"error":{"message":"Upstream request failed"}}`)
				return
			}
			writeSSE(writer, anthropicText(inPlaceHandoff))
		})
		done := make(chan error, 1)
		go func() { done <- compactNow(t, f, x, binding) }()
		clock := f.r.clock.(*manualClock)
		for {
			select {
			case wait := <-clock.waits:
				close(wait.advance)
				continue
			case err := <-done:
				if err != nil {
					t.Fatalf("a native gateway error failed the compaction: %v", err)
				}
			}
			break
		}
		if _, checkpoint := latestCheckpoint(t, f); checkpoint.Native != nil || !checkpoint.Readable() {
			t.Fatal("a native gateway error did not fall back to Sailry's handoff")
		}
		if calls := len(provider.all(compacting)); calls != 2 {
			t.Fatalf("native compaction was sent %d times, want one resend", calls)
		}
	})
	t.Run("a rate limit waits as other requests do", func(t *testing.T) {
		var mu sync.Mutex
		limited := 3
		f, x, binding, provider := nativeFixture(t, protocol.ConversationModelApiAnthropic, func(writer http.ResponseWriter, request wireRequest) {
			mu.Lock()
			fail := compacting(request) && limited > 0
			if fail {
				limited--
			}
			mu.Unlock()
			if fail {
				writeJSON(writer, http.StatusTooManyRequests, `{"type":"error","error":{"type":"rate_limit_error","message":"Rate limited"}}`)
				return
			}
			if compacting(request) {
				writeSSE(writer, anthropicCompaction(`"Waited summary"`, "compaction"))
				return
			}
			writeSSE(writer, anthropicText(inPlaceHandoff))
		})
		done := make(chan error, 1)
		go func() { done <- compactNow(t, f, x, binding) }()
		clock := f.r.clock.(*manualClock)
		for {
			select {
			case wait := <-clock.waits:
				close(wait.advance)
				continue
			case err := <-done:
				if err != nil {
					t.Fatalf("a rate limit failed the native compaction: %v", err)
				}
			}
			break
		}
		if _, checkpoint := latestCheckpoint(t, f); checkpoint.Native == nil {
			t.Fatal("a rate-limited native compaction did not complete natively")
		}
		if calls := len(provider.all(compacting)); calls != 4 {
			t.Fatalf("native compaction was sent %d times, want three resends", calls)
		}
	})
	t.Run("an overload that outlasts the retries ends the compaction", func(t *testing.T) {
		f, x, binding, provider := nativeFixture(t, protocol.ConversationModelApiAnthropic, func(writer http.ResponseWriter, request wireRequest) {
			if compacting(request) {
				writeJSON(writer, 529, `{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}`)
				return
			}
			writeSSE(writer, anthropicText(inPlaceHandoff))
		})
		done := make(chan error, 1)
		go func() { done <- compactNow(t, f, x, binding) }()
		clock := f.r.clock.(*manualClock)
		var err error
		for waiting := true; waiting; {
			select {
			case wait := <-clock.waits:
				close(wait.advance)
			case err = <-done:
				waiting = false
			}
		}
		if failure := model.Classify(err); err == nil || !failure.Exhausted {
			t.Fatalf("an exhausted overload did not end the compaction: %v", err)
		}
		if calls := len(provider.all(compacting)); calls != model.MainRetries+1 {
			t.Fatalf("native compaction was sent %d times, want the ordinary budget", calls)
		}
		if summaries := provider.all(func(request wireRequest) bool { return !compacting(request) }); len(summaries) != 0 {
			t.Fatalf("an exhausted overload fell back to %d summary requests", len(summaries))
		}
	})
	t.Run("a transient failure is retried and leaves the route supported", func(t *testing.T) {
		var mu sync.Mutex
		failures := 1
		f, x, binding, provider := nativeFixture(t, protocol.ConversationModelApiAnthropic, func(writer http.ResponseWriter, request wireRequest) {
			mu.Lock()
			fail := compacting(request) && failures > 0
			if fail {
				failures--
			}
			mu.Unlock()
			if fail {
				writeJSON(writer, http.StatusServiceUnavailable, `{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}`)
				return
			}
			if compacting(request) {
				writeSSE(writer, anthropicCompaction(`"Recovered summary"`, "compaction"))
				return
			}
			writeSSE(writer, anthropicText(inPlaceHandoff))
		})
		done := make(chan error, 1)
		go func() { done <- compactNow(t, f, x, binding) }()
		clock := f.r.clock.(*manualClock)
		select {
		case wait := <-clock.waits:
			close(wait.advance)
		case err := <-done:
			t.Fatalf("compaction ended without a retry: %v", err)
		}
		if err := <-done; err != nil {
			t.Fatal(err)
		}
		if _, checkpoint := latestCheckpoint(t, f); checkpoint.Native == nil {
			t.Fatal("the retried native compaction was not used")
		}
		rejected := false
		f.r.nativeRejected.Range(func(any, any) bool { rejected = true; return false })
		if rejected || len(provider.all(compacting)) != 2 {
			t.Fatal("a transient failure marked the route as unsupported")
		}
	})
}

func TestResponsesNativeCompaction(t *testing.T) {
	t.Run("the compacted window carries the next request", func(t *testing.T) {
		f, x, binding, provider := nativeFixture(t, protocol.ConversationModelApiResponses, func(writer http.ResponseWriter, request wireRequest) {
			if compacting(request) {
				writeJSON(writer, http.StatusOK, compactedWindow)
				return
			}
			writeSSE(writer, responsesText("Done"))
		})
		if err := compactNow(t, f, x, binding); err != nil {
			t.Fatal(err)
		}
		sent := provider.all(compacting)
		if len(sent) != 1 || sent[0].body["tools"] != nil || sent[0].body["store"] != nil || sent[0].body["input"] == nil || sent[0].body["model"] != "fixture" {
			t.Fatalf("compact request = %v", sent)
		}
		_, checkpoint := latestCheckpoint(t, f)
		if checkpoint.Readable() || checkpoint.Native == nil || !strings.Contains(string(checkpoint.Native.Items), "opaque-window") {
			t.Fatal("the opaque window was not stored without a readable summary")
		}
		if usage := checkpoint.Summary.Usage; usage == nil || usage.Total == nil || *usage.Total != 120 {
			t.Fatal("compaction usage was not recorded")
		}
		next := sendMain(t, f, x, binding, provider)
		input := next.body["input"].([]any)
		found, status, plain := false, false, false
		for _, item := range input {
			object := item.(map[string]any)
			found = found || object["type"] == "compaction" && object["encrypted_content"] == "opaque-window" && object["id"] == "cmp_item"
			status = status || object["status"] == "completed"
			plain = plain || object["content"] == "Plain <note>"
		}
		if !found || !status || !plain || strings.Count(next.raw, "Original task") != 1 || !strings.Contains(next.raw, `"Plain <note>"`) {
			t.Fatalf("next request did not send the compacted window as returned: %s", next.raw)
		}
	})
	t.Run("the next native compaction carries the previous window", func(t *testing.T) {
		f, x, binding, provider := nativeFixture(t, protocol.ConversationModelApiResponses, func(writer http.ResponseWriter, request wireRequest) {
			if compacting(request) {
				writeJSON(writer, http.StatusOK, compactedWindow)
				return
			}
			writeSSE(writer, responsesText("Done"))
		})
		if err := compactNow(t, f, x, binding); err != nil {
			t.Fatal(err)
		}
		addExchange(t, f, "Second request")
		if err := compactNow(t, f, x, binding); err != nil {
			t.Fatal(err)
		}
		sent := provider.all(compacting)
		if len(sent) != 2 || !strings.Contains(sent[1].raw, "opaque-window") || !strings.Contains(sent[1].raw, "Second request") {
			t.Fatalf("the second compaction dropped the previous window: %v", sent)
		}
		if summaries := provider.all(func(request wireRequest) bool { return !compacting(request) }); len(summaries) != 0 {
			t.Fatalf("the second compaction fell back to a transcript: %d requests", len(summaries))
		}
	})
	t.Run("a transcript over an opaque window writes its handoff first", func(t *testing.T) {
		f, x, binding, provider := nativeFixture(t, protocol.ConversationModelApiResponses, func(writer http.ResponseWriter, request wireRequest) {
			if compacting(request) {
				writeJSON(writer, http.StatusOK, compactedWindow)
				return
			}
			writeSSE(writer, responsesText(inPlaceHandoff))
		})
		if err := compactNow(t, f, x, binding); err != nil {
			t.Fatal(err)
		}
		addExchange(t, f, "Second request")
		// A focus keeps the request on Sailry's handoff.
		x.inv.Message.Text = "Keep the second request"
		if err := compactNow(t, f, x, binding); err != nil {
			t.Fatal(err)
		}
		summaries := provider.all(func(request wireRequest) bool { return !compacting(request) })
		if len(summaries) != 2 || !strings.Contains(summaries[0].raw, "Original task") || !strings.Contains(summaries[1].raw, "Continue the task") || !strings.Contains(summaries[1].raw, "Second request") {
			t.Fatalf("the focused compaction did not summarize over the previous handoff: %d requests", len(summaries))
		}
	})
	t.Run("a missing endpoint falls back and is remembered", func(t *testing.T) {
		f, x, binding, provider := nativeFixture(t, protocol.ConversationModelApiResponses, func(writer http.ResponseWriter, request wireRequest) {
			if compacting(request) {
				writeJSON(writer, http.StatusNotFound, `{"error":{"message":"Not found","type":"invalid_request_error"}}`)
				return
			}
			writeSSE(writer, responsesText(inPlaceHandoff))
		})
		if err := compactNow(t, f, x, binding); err != nil {
			t.Fatal(err)
		}
		if _, checkpoint := latestCheckpoint(t, f); checkpoint.Native != nil || !checkpoint.Readable() {
			t.Fatal("a missing compact endpoint did not fall back to Sailry's handoff")
		}
		addExchange(t, f, "Second request")
		if err := compactNow(t, f, x, binding); err != nil {
			t.Fatal(err)
		}
		if len(provider.all(compacting)) != 1 {
			t.Fatal("a route without the compact endpoint was asked again")
		}
	})
	t.Run("another model gets a readable handoff written lazily", func(t *testing.T) {
		f, x, binding, provider := nativeFixture(t, protocol.ConversationModelApiResponses, func(writer http.ResponseWriter, request wireRequest) {
			if compacting(request) {
				writeJSON(writer, http.StatusOK, compactedWindow)
				return
			}
			writeSSE(writer, responsesText(inPlaceHandoff))
		})
		if err := compactNow(t, f, x, binding); err != nil {
			t.Fatal(err)
		}
		_, opaque := latestCheckpoint(t, f)
		languageModel, err := model.New(f.ctx, binding)
		if err != nil {
			t.Fatal(err)
		}
		route := model.RouteOf(languageModel, f.session.ID)
		// The producing route reads the window itself and needs no text.
		if written, err := f.r.readableCheckpoint(x, binding, route, f.state(), false); err != nil || written {
			t.Fatalf("the producing route wrote a handoff: %v %v", written, err)
		}
		other := withModel(binding, "other")
		if written, err := f.r.readableCheckpoint(x, other, route, f.state(), false); err != nil || !written {
			t.Fatalf("switching models did not write the handoff: %v %v", written, err)
		}
		summaries := provider.all(func(request wireRequest) bool { return !compacting(request) })
		if len(summaries) != 1 || summaries[0].body["model"] != "other" || !strings.Contains(summaries[0].raw, "Original task") {
			t.Fatal("the lazy handoff did not summarize the replaced history on the new model")
		}
		_, readable := latestCheckpoint(t, f)
		if !readable.Readable() || readable.Boundary != opaque.Boundary || readable.Native == nil || string(readable.Native.Items) != string(opaque.Native.Items) {
			t.Fatal("the readable checkpoint lost its boundary or the window")
		}
		if written, err := f.r.readableCheckpoint(x, other, route, f.state(), false); err != nil || written {
			t.Fatal("a readable checkpoint was summarized again")
		}
		if next := sendMain(t, f, x, other, provider); strings.Contains(next.raw, "opaque-window") || !strings.Contains(next.raw, "Continue the task") {
			t.Fatalf("another model did not read the handoff: %s", next.raw)
		}
		if next := sendMain(t, f, x, binding, provider); !strings.Contains(next.raw, "opaque-window") || strings.Contains(next.raw, "Continue the task") {
			t.Fatalf("the producing model did not keep its window: %s", next.raw)
		}
	})
}
