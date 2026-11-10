package plugins

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func cardTestFixture(t *testing.T) (protocol.PluginInfo, protocol.PluginContext) {
	t.Helper()
	info := protocol.DefaultPluginInfo()
	info.Summary.Name, info.Summary.Digest = "card-fixture", strings.Repeat("a", 64)
	var extension protocol.PluginExtension
	if err := json.Unmarshal([]byte(`{
		"api_version":"v1","actions":[],
		"host":{"entry":"dev.sailry.platform/host.js","resources":["dev.sailry.platform/host.js"],"handlers":["connection"]},
		"desktop":{"resources":["dev.sailry.platform/card.js"]},
		"tools":[{"name":"connection","presentation":"content","handler":{"name":"connection","read_only":true,"parameters":{"type":"object","properties":{},"additionalProperties":false}}}],
		"cards":[{"key":"connection","entry":"dev.sailry.platform/card.js","tool":"connection","mcp_server":"input",
			"props_schema":{"type":"object","additionalProperties":false,"properties":{"configured":{"type":"boolean"},"revision":{"type":"integer"}},"required":["configured"]},
			"capabilities":{"browser":[{"name":"login","origins":["https://example.test"],"source":{"kind":"mcp_authorization"},"modes":["external"]}]}}]
	}`), &extension); err != nil {
		t.Fatal(err)
	}
	info.Extension = &extension
	scope := protocol.DefaultPluginContext()
	session, turn, worktree := protocol.NewSessionID(), protocol.NewTurnID(), protocol.NewWorktreeID()
	scope.Package, scope.Session, scope.Turn, scope.Worktree = PackageReference(info), &session, &turn, &worktree
	return info, scope
}

func testCardContracts(t *testing.T) {
	t.Run("canonical ownership and original failures", func(t *testing.T) {
		info, scope := cardTestFixture(t)
		if err := ValidateCards(*info.Extension); err != nil {
			t.Fatal(err)
		}
		input := json.RawMessage(`{"isError":true,"error":{"code":"not_configured","message":"Connect"},"sailry_content":{"version":1,"blocks":[{"kind":"card","key":"connection","props":{"configured":false,"revision":9007199254740993},"fallback":{"label":"Connect"}}]}}`)
		call := ToolCallIdentity(*scope.Turn, scope.Package, ToolAlias(info.Summary.Name, "connection"), "call")
		body := StampCards(input, protocol.NodeID{1}, scope, info, call)
		var result struct {
			IsError bool                 `json:"isError"`
			Error   json.RawMessage      `json:"error"`
			Content protocol.ToolContent `json:"sailry_content"`
		}
		if err := json.Unmarshal(body, &result); err != nil {
			t.Fatal(err)
		}
		if !result.IsError || string(result.Error) != `{"code":"not_configured","message":"Connect"}` || len(result.Content.Blocks) != 1 {
			t.Fatal("card stamping changed the authoritative tool failure")
		}
		summary, ok := result.Content.Blocks[0].Data.(*protocol.PluginCardSummary)
		if !ok || !CardSummaryValid(*summary) || summary.Node != (protocol.NodeID{1}) ||
			summary.Receipt != (protocol.PluginCardReceipt{Session: *scope.Session, Turn: *scope.Turn, Call: call, Block: 0}) ||
			summary.Worktree != *scope.Worktree || summary.Package != scope.Package || !strings.Contains(string(summary.Props), "9007199254740993") {
			t.Fatal("card ownership or exact numeric props were not captured")
		}
		if string(StampCards(json.RawMessage(`{"original":9007199254740993}`), protocol.NodeID{1}, scope, info, call)) != `{"original":9007199254740993}` {
			t.Fatal("a result without cards changed during stamping")
		}
	})
	t.Run("forged or invalid drafts discard only presentation", func(t *testing.T) {
		info, scope := cardTestFixture(t)
		for _, block := range []string{
			`{"kind":"card","key":"connection","props":{"configured":false},"fallback":{"label":"Connect"},"receipt":{}}`,
			`{"kind":"card","key":"unknown","props":{"configured":false},"fallback":{"label":"Connect"}}`,
			`{"kind":"card","key":"connection","props":{"configured":false,"token":"private"},"fallback":{"label":"Connect"}}`,
			`{"kind":"card","key":"connection","props":{"configured":"false"},"fallback":{"label":"Connect"}}`,
			`{"kind":"card","key":"connection","props":{"configured":false},"fallback":{"label":" "}}`,
		} {
			input := json.RawMessage(`{"isError":true,"error":{"code":"not_configured","message":"Connect"},"sailry_content":{"version":1,"blocks":[` + block + `]}}`)
			body := StampCards(input, protocol.NodeID{1}, scope, info, protocol.NewRequestID())
			var result map[string]json.RawMessage
			if err := json.Unmarshal(body, &result); err != nil {
				t.Fatal(err)
			}
			if _, exists := result["sailry_content"]; exists || string(result["isError"]) != "true" ||
				string(result["error"]) != `{"code":"not_configured","message":"Connect"}` {
				t.Fatal("invalid card retained authority or removed the original failure")
			}
		}
		input := json.RawMessage(`{"original":true,"sailry_content":{"version":1,"blocks":[{"kind":"card","key":"connection","props":{"configured":false},"fallback":{"label":"Connect"}}]}}`)
		for _, missing := range []string{"session", "turn", "worktree"} {
			captured := scope
			switch missing {
			case "session":
				captured.Session = nil
			case "turn":
				captured.Turn = nil
			case "worktree":
				captured.Worktree = nil
			}
			if strings.Contains(string(StampCards(input, protocol.NodeID{1}, captured, info, protocol.NewRequestID())), "sailry_content") {
				t.Fatalf("card was stamped without captured %s", missing)
			}
		}
		blocks := strings.Repeat(`{"kind":"notice","message":{"label":"Notice"}},`, MaxCards)
		blocks += `{"kind":"card","key":"connection","props":{"configured":false},"fallback":{"label":"Connect"}}`
		tooMany := json.RawMessage(`{"original":true,"sailry_content":{"version":1,"blocks":[` + blocks + `]}}`)
		if strings.Contains(string(StampCards(tooMany, protocol.NodeID{1}, scope, info, protocol.NewRequestID())), "sailry_content") {
			t.Fatal("card after the sixteenth block was stamped")
		}
	})
	t.Run("bounded closed scalar schemas and declared capabilities", func(t *testing.T) {
		for _, scenario := range []string{
			"missing tool", "write handler", "wrong presentation", "write operation", "write flow", "missing settings",
			"undeclared entry", "duplicate card", "unknown pointer", "http origin", "origin path", "origin query",
			"default port origin", "duplicate mode", "open schema", "private property", "nested property",
			"unbounded string", "external schema",
		} {
			t.Run(scenario, func(t *testing.T) {
				info, _ := cardTestFixture(t)
				extension := info.Extension
				card := &extension.Cards[0]
				switch scenario {
				case "missing tool":
					*card.Tool = "missing"
				case "write handler":
					extension.Tools[0].Handler.ReadOnly = false
				case "wrong presentation":
					extension.Tools[0].Presentation = protocol.ToolPresentationDetails
				case "write operation":
					operation := protocol.ToolOperationSetValue
					extension.Tools[0].Handler.Operation = &operation
				case "write flow":
					extension.Tools[0].Handler.Flow = &protocol.ToolFlow{Operations: []protocol.ToolOperation{protocol.ToolOperationSetValue}}
				case "missing settings":
					card.Capabilities.Settings = true
				case "undeclared entry":
					card.Entry = "dev.sailry.platform/missing.js"
				case "duplicate card":
					extension.Cards = append(extension.Cards, *card)
				case "unknown pointer":
					card.Capabilities.Qr = &protocol.PluginCardQr{Payload: "/missing"}
				case "http origin":
					card.Capabilities.Browser[0].Origins = []string{"http://example.test"}
				case "origin path":
					card.Capabilities.Browser[0].Origins = []string{"https://example.test/login"}
				case "origin query":
					card.Capabilities.Browser[0].Origins = []string{"https://example.test?token=private"}
				case "default port origin":
					card.Capabilities.Browser[0].Origins = []string{"https://example.test:443"}
				case "duplicate mode":
					card.Capabilities.Browser[0].Modes = []protocol.PluginCardMode{"external", "external"}
				case "open schema":
					card.PropsSchema = json.RawMessage(`{"type":"object","additionalProperties":true,"properties":{}}`)
				case "private property":
					card.PropsSchema = json.RawMessage(`{"type":"object","additionalProperties":false,"properties":{"access_token":{"type":"string","maxLength":64}}}`)
				case "nested property":
					card.PropsSchema = json.RawMessage(`{"type":"object","additionalProperties":false,"properties":{"value":{"type":"object"}}}`)
				case "unbounded string":
					card.PropsSchema = json.RawMessage(`{"type":"object","additionalProperties":false,"properties":{"value":{"type":"string"}}}`)
				case "external schema":
					card.PropsSchema = json.RawMessage(`{"type":"object","additionalProperties":false,"properties":{"value":{"type":"string","maxLength":64,"$ref":"https://example.test/schema"}}}`)
				}
				if ValidateCards(*extension) == nil {
					t.Fatal("invalid card declaration was accepted")
				}
			})
		}
		info, _ := cardTestFixture(t)
		info.Extension.Cards[0].Tool = nil
		if err := ValidateCards(*info.Extension); err != nil {
			t.Fatal("optional card tool metadata became required")
		}
	})
	t.Run("scalar props follow source integer and string rules", func(t *testing.T) {
		declaration := protocol.PluginCardDeclaration{PropsSchema: json.RawMessage(`{"type":"object","additionalProperties":false,"properties":{"label":{"type":"string","minLength":1,"maxLength":2},"count":{"type":"integer","minimum":-1,"maximum":9007199254740993},"enabled":{"type":"boolean","enum":[true]}},"required":["label","count","enabled"]}`)}
		for _, body := range []string{
			`{"label":"🙂","count":9007199254740993,"enabled":true}`,
			`{"label":"ok","count":-1,"enabled":true}`,
		} {
			if !CardAccepts(declaration, json.RawMessage(body)) {
				t.Fatal("valid source scalar props were rejected")
			}
		}
		for _, body := range []string{
			`{"label":"ok","count":1.0,"enabled":true}`,
			`{"label":"ok","count":9007199254740994,"enabled":true}`,
			`{"label":"","count":1,"enabled":true}`,
			`{"label":"long","count":1,"enabled":true}`,
			`{"label":"ok","count":1,"enabled":false}`,
			`{"label":"ok","count":1,"enabled":true,"unknown":false}`,
		} {
			if CardAccepts(declaration, json.RawMessage(body)) {
				t.Fatal("invalid source scalar props were accepted")
			}
		}
		declaration.PropsSchema = json.RawMessage(`{"type":"object","additionalProperties":false,"properties":{"url":{"type":"string","maxLength":4096}}}`)
		if CardAccepts(declaration, json.RawMessage(`{"url":"https://example.test?access_token=private"}`)) {
			t.Fatal("authorization URL entered public card props")
		}
	})
}
