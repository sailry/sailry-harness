package live

import (
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
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
		state := client.RequestFailed
		if code == protocol.ErrorCodeOutcomeUnknown {
			// Recorded by the Node as never to be known: final as well.
			state = client.RequestUnknown
		}
		_, err, confirmed := pluginCardInspected(client.Resolution{State: state, Fault: failure}, nil)
		if !confirmed || err == nil || faultCode(err) != code {
			t.Fatal("completed card fault was not definitive")
		}
	}
	// Only a request the Node still runs, or a Node that cannot be asked,
	// keeps the login unconfirmed; every other answer ends the request, so a
	// card never asks about the same request forever.
	for name, inspected := range map[string]struct {
		resolution client.Resolution
		failure    error
		confirmed  bool
		code       protocol.ErrorCode
	}{
		"running":       {client.Resolution{State: client.RequestPending}, nil, false, protocol.ErrorCodeOutcomeUnknown},
		"unreachable":   {client.Resolution{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "Node is unreachable"}, false, protocol.ErrorCodeUnavailable},
		"interrupted":   {client.Resolution{State: client.RequestUnknown, Fault: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "the Node restarted"}}, nil, true, protocol.ErrorCodeOutcomeUnknown},
		"never arrived": {client.Resolution{State: client.RequestAbsent}, nil, true, protocol.ErrorCodeUnavailable},
		"applied":       {client.Resolution{State: client.RequestApplied, Output: protocol.Output{Kind: "connector_login"}}, nil, true, ""},
	} {
		output, err, confirmed := pluginCardInspected(inspected.resolution, inspected.failure)
		if confirmed != inspected.confirmed || faultCode(err) != inspected.code || (err == nil) != (output.Kind == "connector_login") {
			t.Fatalf("%s card inspection = %#v, %v, confirmed %t", name, output, err, confirmed)
		}
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
