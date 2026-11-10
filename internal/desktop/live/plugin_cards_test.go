package live

import (
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func testPluginCardContracts(t *testing.T) {
	target := protocol.PluginCardTarget{Origins: []string{"https://example.test"}}
	redirect := "http://127.0.0.1:43191/callback"
	for _, current := range []struct {
		address string
		valid   bool
	}{
		{"https://example.test/authorize?state=private", true},
		{"https://example.test:443/authorize", true},
		{"https://example.test.evil/authorize", false},
		{"https://user@example.test/authorize", false},
		{"http://example.test/authorize", false},
		{"javascript:alert(1)", false},
		{redirect + "?code=private&state=private", true},
		{"http://127.0.0.1:43192/callback?code=private", false},
		{"http://127.0.0.1:43191/other?code=private", false},
		{redirect + "#private", false},
	} {
		if pluginCardBrowserAllowed(current.address, target, &redirect) != current.valid {
			t.Fatalf("card browser origin or callback confinement changed: %s", current.address)
		}
	}
	request := protocol.NewRequest(protocol.NodeID{1}, protocol.Command{Kind: "cancel_connector_login", Data: protocol.CommandCancelConnectorLoginPayload{Attempt: protocol.NewRequestID()}})
	login := pluginCardAuthorization{
		request: &request, state: "pending", url: "https://example.test/authorize?state=private", expires: 101,
		redirect: &redirect, returned: make(chan string),
	}
	if !login.canOpen(time.UnixMilli(100)) || login.canOpen(time.UnixMilli(101)) {
		t.Fatal("card authorization URL ignored its exact expiry")
	}
	login.suspend()
	if login.request == nil || login.request.ID != request.ID || login.url != "" || login.redirect != nil ||
		login.returned != nil || login.state != "unconfirmed" || login.keepsAlive(time.UnixMilli(100)) {
		t.Fatal("card suspension lost the original uncertain request or retained private callback state")
	}
	for _, code := range []protocol.ErrorCode{protocol.ErrorCodeOutcomeUnknown, protocol.ErrorCodeUnavailable, protocol.ErrorCodeBusy, protocol.ErrorCodeInternal, protocol.ErrorCodeCancelled} {
		if !pluginCardUncertain(code) {
			t.Fatalf("card lost an uncertain durable request: %s", code)
		}
		failure := &protocol.Fault{Code: code, Message: "isolated completed outcome"}
		outcome := protocol.RequestOutcome{Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{Err: failure}}
		_, err, confirmed := pluginCardInspected(outcome, nil)
		if !confirmed || err == nil || faultCode(err) != code {
			t.Fatal("completed card fault was not definitive")
		}
	}
	if _, err, confirmed := pluginCardInspected(protocol.RequestOutcome{Kind: "not_admitted"}, nil); confirmed || faultCode(err) != protocol.ErrorCodeOutcomeUnknown {
		t.Fatal("card inspection replayed an uncertain action")
	}
	reference := protocol.PluginReference{Name: "captured-card", Digest: strings.Repeat("a", 64), SettingsRevision: 2}
	card := &pluginCardInstance{summary: protocol.PluginCardSummary{Package: reference}, reference: reference}
	replacement := reference
	replacement.Digest, replacement.SettingsRevision = strings.Repeat("b", 64), 20
	card.updateReference(replacement)
	if card.scope().Package != reference {
		t.Fatal("card capture followed the current inventory replacement")
	}
	replacement = reference
	replacement.SettingsRevision++
	card.updateReference(replacement)
	if card.scope().Package != replacement || card.summary.Package != reference {
		t.Fatal("card settings reference mutated the canonical captured summary")
	}
}
