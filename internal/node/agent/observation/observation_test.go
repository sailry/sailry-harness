package observation

import (
	"encoding/json"
	"reflect"
	"strings"
	"testing"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestRequestComposition(t *testing.T) {
	t.Parallel()
	decode := func(t *testing.T, raw string) any {
		t.Helper()
		var value any
		if err := json.Unmarshal([]byte(raw), &value); err != nil {
			t.Fatal(err)
		}
		return value
	}
	encode := func(t *testing.T, value any) []byte {
		t.Helper()
		raw, err := json.Marshal(value)
		if err != nil {
			t.Fatal(err)
		}
		return raw
	}
	row := func(t *testing.T, rows []protocol.ConversationObservationCategory, kind protocol.ConversationObservationCategoryKind) protocol.ConversationObservationCategory {
		t.Helper()
		for _, row := range rows {
			if row.Kind == kind {
				return row
			}
		}
		t.Fatalf("missing category %s", kind)
		return protocol.ConversationObservationCategory{}
	}
	assertTokens := func(t *testing.T, rows []protocol.ConversationObservationCategory, kind protocol.ConversationObservationCategoryKind, expected uint64) {
		t.Helper()
		value := row(t, rows, kind)
		if value.Tokens == nil || *value.Tokens != expected {
			t.Fatalf("%s tokens = %v; want %d", kind, value.Tokens, expected)
		}
	}
	unknown := func(t *testing.T, rows []protocol.ConversationObservationCategory, kinds ...protocol.ConversationObservationCategoryKind) {
		t.Helper()
		for _, kind := range kinds {
			value := row(t, rows, kind)
			if value.Tokens != nil || value.Items != nil || value.Method != nil {
				t.Fatalf("%s unexpectedly known: %+v", kind, value)
			}
		}
	}
	quarter := func(text string) uint64 { return (uint64(utf8.RuneCountInString(text)) + 3) / 4 }

	t.Run("inventory metadata roundtrip", func(t *testing.T) {
		current := NewContext([]Source{{Kind: injected, Text: "Current state 中文 🙂"}, {Kind: skillsKind, Text: "Current skill"}})
		previous := PersistedContext(NewContext([]Source{{Kind: skillsKind, Text: "Earlier skill"}}).Text(), []string{"Earlier skill"})
		inventory := Inventory{
			System: "Base Policy fragment", Sources: []Source{{Kind: injected, Text: "Policy fragment"}},
			Contexts: []Context{previous, *current},
		}
		var restored Inventory
		if err := json.Unmarshal(encode(t, &inventory), &restored); err != nil {
			t.Fatal(err)
		}
		if !reflect.DeepEqual(inventory, restored) {
			t.Fatal("inventory metadata lost private context ranges or persistence")
		}
		body := map[string]any{"system": inventory.System, "messages": []any{
			map[string]any{"role": "user", "content": previous.Text()},
			map[string]any{"role": "assistant", "content": "reply"},
			map[string]any{"role": "user", "content": current.Text()},
		}}
		before, after := Prepared(encode(t, body), &inventory), Prepared(encode(t, body), &restored)
		if !reflect.DeepEqual(before, after) {
			t.Fatal("metadata roundtrip changed prepared request attribution")
		}
		assertTokens(t, after.Categories, skillsKind, quarter("Earlier skillCurrent skill"))
	})

	t.Run("wrapped tool declarations", func(t *testing.T) {
		declaration := `{"name":"read","description":"file","parameters":{"type":"object"}}`
		for _, tools := range []string{"[" + declaration + "]", `{"functionDeclarations":[` + declaration + "]}", `{"function_declarations":[` + declaration + "]}"} {
			rows := estimate(decode(t, `{"contents":[],"tools":`+tools+"}"), nil, false, nil)
			assertTokens(t, rows, toolsKind, 4)
			if value := row(t, rows, toolsKind); value.Items == nil || *value.Items != 1 {
				t.Fatalf("tools items = %v", value.Items)
			}
		}
		rows := estimate(decode(t, `{"contents":[],"tools":{"opaque":"declarations"}}`), nil, false, nil)
		unknown(t, rows, kinds[:]...)
	})

	t.Run("assistant siblings", func(t *testing.T) {
		for _, content := range []struct {
			value any
			chars uint64
		}{{nil, 0}, {"", 0}, {"reply", 5}, {[]any{map[string]any{"type": "text", "text": "reply"}}, 9}} {
			body := map[string]any{"messages": []any{map[string]any{
				"role": "assistant", "content": content.value,
				"tool_calls":        []any{map[string]any{"id": "call", "type": "function", "function": map[string]any{"name": "read", "arguments": "{}"}}},
				"reasoning_content": "think",
			}}}
			rows := estimate(body, nil, false, nil)
			assertTokens(t, rows, assistantKind, (23+content.chars+3)/4)
		}
		rows := estimate(decode(t, `{"messages":[{"role":"assistant","function_call":{"name":"read","arguments":"{}"},"reasoning_content":"think"}]}`), nil, false, nil)
		assertTokens(t, rows, assistantKind, 5)
	})

	t.Run("results and opaque media", func(t *testing.T) {
		body := decode(t, `{"messages":[{"role":"user","content":[{"type":"text","text":"hello"},{"type":"tool_result","content":"completed"},{"type":"image","source":{"data":"SECRET"}}]}]}`)
		rows := estimate(body, nil, false, nil)
		if value := row(t, rows, resultsKind); value.Items == nil || *value.Items != 1 {
			t.Fatalf("result items = %v", value.Items)
		}
		unknown(t, rows, skillsKind)
		unknown(t, estimate(decode(t, `{"unknown":"private"}`), nil, false, nil), kinds[:]...)
		clean := estimate(decode(t, `{"input":[{"role":"user","content":[{"text":"ok","data":"PRIVATE","image_url":"PRIVATE","file_data":"PRIVATE","signature":"PRIVATE","thoughtSignature":"PRIVATE","encrypted_content":"PRIVATE","url":"data:PRIVATE"}]}]}`), nil, false, nil)
		assertTokens(t, clean, userKind, 1)
		for _, raw := range []string{
			`{"contents":[{"role":"user","parts":[{"functionResponse":{"name":"read","response":{"output":"done"}}}]}]}`,
			`{"input":[{"type":"function_call_output","output":"done"}]}`,
			`{"input":[{"type":"computer_call_output","output":"done"}]}`,
			`{"messages":[{"role":"function","content":"done"}]}`,
		} {
			if value := row(t, estimate(decode(t, raw), nil, false, nil), resultsKind); value.Items == nil || *value.Items != 1 {
				t.Fatalf("result carrier unrecognized: %s", raw)
			}
		}
	})

	t.Run("present fragments and missing provenance", func(t *testing.T) {
		sources := []Source{{Kind: injected, Text: "injected fragment"}, {Kind: skillsKind, Text: "skill fragment"}}
		rows := estimate(decode(t, `{"instructions":"base injected fragment skill fragment"}`), sources, true, nil)
		for _, kind := range []protocol.ConversationObservationCategoryKind{injected, skillsKind} {
			if value := row(t, rows, kind); value.Items == nil || *value.Items != 1 {
				t.Fatalf("%s items = %v", kind, value.Items)
			}
		}
		omitted := estimate(decode(t, `{"instructions":"rewritten upstream"}`), sources, true, nil)
		unknown(t, omitted, skillsKind, injected)
		body := decode(t, `{"instructions":"base","input":[]}`)
		for _, kind := range []protocol.ConversationObservationCategoryKind{skillsKind, injected} {
			assertTokens(t, estimate(body, nil, true, nil), kind, 0)
			unknown(t, estimate(body, nil, false, nil), kind)
		}
		sources = []Source{{Kind: skillsKind, Text: "skill fragment"}}
		quoted := estimate(decode(t, `{"instructions":"base","input":[{"role":"user","content":"skill fragment"}]}`), sources, true, nil)
		unknown(t, quoted, skillsKind)
		assertTokens(t, quoted, userKind, 4)
		sources = append(sources, Source{Kind: skillsKind, Text: "missing fragment"})
		unknown(t, estimate(decode(t, `{"instructions":"skill fragment"}`), sources, true, nil), skillsKind)
		unknown(t, estimate(decode(t, `{"input":null}`), nil, true, nil), kinds[:]...)
	})

	t.Run("terminal context ranges", func(t *testing.T) {
		context := NewContext([]Source{{Kind: injected, Text: "Current state 中文 🙂"}, {Kind: skillsKind, Text: "Exact skill body"}})
		sources := []Source{{Kind: injected, Text: "Policy fragment"}}
		quotation := "Quote Policy fragment and Exact skill body; <turn-context>\nOlder state\n</turn-context>"
		for _, raw := range contextBodies(quotation, context.Text(), false) {
			body := decode(t, raw)
			before := encode(t, body)
			rows := estimate(body, sources, true, []Context{*context})
			assertTokens(t, rows, skillsKind, 4)
			assertTokens(t, rows, injected, quarter("Policy fragment"+strings.ReplaceAll(context.Text(), "Exact skill body", "")))
			if value := row(t, rows, userKind); value.Tokens == nil || *value.Tokens < quarter(quotation) {
				t.Fatalf("quoted text misattributed: %+v", value)
			}
			if !reflect.DeepEqual(before, encode(t, body)) {
				t.Fatal("classification changed request")
			}
		}
	})

	t.Run("rejects rewritten and ambiguous contexts", func(t *testing.T) {
		current := NewContext([]Source{{Kind: skillsKind, Text: "Full skill"}})
		quoted := string(encode(t, current.Text()))
		rewritten := []string{
			`{"system":"base","messages":[{"role":"user","content":"rewritten"}]}`,
			`{"system":"base","messages":[{"role":"user","content":` + quoted + `},{"role":"user","content":"later input"}]}`,
			`{"system":"base","messages":[{"role":"user","content":` + string(encode(t, current.Text()+" altered")) + `}]}`,
			`{"system":"base","messages":[{"role":"user","content":[{"type":"text","text":` + quoted + `},{"type":"image","data":"opaque"}]}]}`,
			`{"system":"base","messages":[],"input":[{"role":"user","content":` + quoted + `}]}`,
			`{"input":{"opaque":` + quoted + `}}`,
		}
		persisted := PersistedContext(NewContext([]Source{{Kind: skillsKind, Text: "Captured skill"}}).Text(), []string{"Captured skill"})
		quoted = string(encode(t, persisted.Text()))
		ambiguous := []string{
			`{"messages":[{"role":"user","content":` + quoted + `},{"role":"user","content":` + quoted + `}]}`,
			`{"messages":[{"role":"user","content":` + string(encode(t, persisted.Text()+"\n"+persisted.Text())) + `}]}`,
			`{"messages":[{"role":"user","content":"rewritten"}]}`,
			`{"messages":[{"role":"user","content":` + string(encode(t, "quoted "+persisted.Text())) + `}]}`,
		}
		for _, scenario := range []struct {
			context Context
			bodies  []string
		}{{*current, rewritten}, {persisted, ambiguous}} {
			for _, raw := range scenario.bodies {
				rows := estimate(decode(t, raw), nil, true, []Context{scenario.context})
				unknown(t, rows, skillsKind, injected)
			}
		}
	})

	t.Run("persisted contexts without reinjection", func(t *testing.T) {
		skills := []string{"First complete skill", "Second complete skill"}
		contexts := make([]Context, 0, len(skills))
		for _, skill := range skills {
			contexts = append(contexts, PersistedContext(NewContext([]Source{{Kind: skillsKind, Text: skill}}).Text(), skills))
		}
		quotation := "User quotes First complete skill"
		joined := contexts[0].Text() + "\n" + contexts[1].Text()
		for _, raw := range contextBodies(quotation, joined, true) {
			rows := estimate(decode(t, raw), nil, true, contexts)
			assertTokens(t, rows, skillsKind, quarter(strings.Join(skills, "")))
			injectedText := strings.ReplaceAll(strings.ReplaceAll(joined, skills[0], ""), skills[1], "")
			// The separator between distinct persisted contexts remains user text.
			assertTokens(t, rows, injected, (uint64(utf8.RuneCountInString(injectedText))-1+3)/4)
			if value := row(t, rows, userKind); value.Tokens == nil || *value.Tokens < quarter(quotation) {
				t.Fatalf("quotation lost: %+v", value)
			}
		}
	})

	t.Run("prepared bodies and continuation completeness", func(t *testing.T) {
		inventory := &Inventory{System: "adapted", Sources: []Source{}}
		raw := []byte(`{"messages":[{"role":"system","content":"adapted"},{"role":"user","content":"sent"},{"role":"tool","content":"result"}],"tools":[{"name":"lookup"}]}`)
		before := append([]byte(nil), raw...)
		result := Prepared(raw, inventory)
		assertTokens(t, result.Categories, systemKind, 2)
		assertTokens(t, result.Categories, userKind, 1)
		assertTokens(t, result.Categories, resultsKind, 2)
		assertTokens(t, result.Categories, skillsKind, 0)
		if !result.Complete || !reflect.DeepEqual(raw, before) {
			t.Fatal("prepared body was incomplete or changed")
		}
		rewritten := Prepared([]byte(`{"messages":[{"role":"system","content":"rewritten"}]}`), inventory)
		assertTokens(t, rewritten.Categories, systemKind, 3)
		unknown(t, rewritten.Categories, skillsKind, injected)
		for _, raw := range []string{"", "not JSON", "{}", `{"private":"value"}`, `{"input":{"prompt":"opaque"}}`, `{"system":"adapted","messages":{"prompt":"opaque"}}`, `{"contents":"opaque"}`} {
			result := Prepared([]byte(raw), inventory)
			unknown(t, result.Categories, kinds[:]...)
			if result.Complete {
				t.Fatalf("unsupported body complete: %s", raw)
			}
		}
		for _, key := range []string{"previous_response_id", "previous_interaction_id"} {
			body := map[string]any{"instructions": "adapted", "input": []any{}, key: "previous"}
			result := Prepared(encode(t, body), inventory)
			if result.Complete {
				t.Fatalf("continuation %s considered complete", key)
			}
			assertTokens(t, result.Categories, skillsKind, 0)
			assertTokens(t, result.Categories, injected, 0)
			body[key] = nil
			if !Prepared(encode(t, body), inventory).Complete {
				t.Fatalf("null continuation %s considered incomplete", key)
			}
		}
		context := NewContext([]Source{{Kind: skillsKind, Text: "captured skill"}})
		contextInventory := &Inventory{System: "adapted", Contexts: []Context{*context}}
		for _, text := range []string{context.Text(), "plugin changed captured skill"} {
			body := map[string]any{"system": "adapted", "messages": []any{map[string]any{"role": "user", "content": text}}}
			prepared := Prepared(encode(t, body), contextInventory)
			if text == context.Text() {
				assertTokens(t, prepared.Categories, skillsKind, 4)
				if value := row(t, prepared.Categories, injected); value.Tokens == nil || *value.Tokens == 0 {
					t.Fatal("context wrapper was not attributed")
				}
			} else {
				unknown(t, prepared.Categories, skillsKind, injected)
			}
			if !prepared.Complete {
				t.Fatal("complete adapted body depends on provenance availability")
			}
		}
		duplicate := &Inventory{System: "base fragment", Sources: []Source{{Kind: skillsKind, Text: "fragment"}, {Kind: skillsKind, Text: "fragment"}}}
		assertTokens(t, Prepared([]byte(`{"instructions":"base fragment"}`), duplicate).Categories, skillsKind, 2)
		encoded := string(encode(t, result))
		for _, secret := range []string{"adapted", "sent", "result\"", "PRIVATE_AUTH", "PRIVATE_RESPONSE"} {
			if strings.Contains(encoded, secret) {
				t.Fatalf("result retained body text %q", secret)
			}
		}
	})
}

func contextBodies(quotation, context string, persisted bool) []string {
	quote, _ := json.Marshal(quotation)
	text, _ := json.Marshal(context)
	joined, _ := json.Marshal(quotation + "\n" + context)
	suffix := ""
	if persisted {
		suffix = `,{"role":"assistant","content":"reply"}`
	}
	return []string{
		`{"messages":[{"role":"system","content":"Base Policy fragment"},{"role":"user","content":` + string(joined) + `}` + suffix + `]}`,
		`{"input":[{"role":"system","content":[{"type":"input_text","text":"Base Policy fragment"}]},{"role":"user","content":[{"type":"input_text","text":` + string(quote) + `},{"type":"input_text","text":` + string(text) + `}]}` + suffix + `]}`,
		`{"system":"Base Policy fragment","messages":[{"role":"user","content":[{"type":"text","text":` + string(quote) + `},{"type":"text","text":` + string(text) + `}]}` + suffix + `]}`,
		`{"systemInstruction":{"parts":[{"text":"Base Policy fragment"}]},"contents":[{"role":"user","parts":[{"text":` + string(quote) + `},{"text":` + string(text) + `}]}` + suffix + `]}`,
	}
}
