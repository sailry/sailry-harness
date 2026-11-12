package live

import (
	"context"
	"net/http"
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestAuthorizationProjectionAndCallbacks(t *testing.T) {
	// The plugin subtests draw with a ui.Tester, whose font atlas every
	// tester shares: they run before this test joins the parallel ones.
	t.Run("canonical card contracts", testPluginCardContracts)
	t.Run("native component authority", testNativeComponentAuthority)
	t.Run("plugin view ownership", testPluginViewOwnership)
	t.Run("plugin diagnostic report", testPluginDiagnostic)
	t.Run("plugin location ownership", testPluginLocationOwnership)
	t.Parallel()
	t.Run("quota remains unknown after reset", func(t *testing.T) {
		reset := uint64(100)
		window := &protocol.ConversationQuotaWindow{UsedBasisPoints: 4250, ResetAtMs: &reset}
		if value := quotaRemaining(window, 99); value != 57.5 {
			t.Fatalf("remaining before reset: %v", value)
		}
		if value := quotaRemaining(window, 100); value != -1 {
			t.Fatalf("expired observation became known: %v", value)
		}
		window.UsedBasisPoints, window.ResetAtMs = 10050, nil
		if value := quotaRemaining(window, 100); value != 0 {
			t.Fatalf("remaining exceeded its lower bound: %v", value)
		}
	})
	t.Run("login states and controller callback ownership", func(t *testing.T) {
		view := settings.Authorization{Open: true}
		expiry, terminal := projectAuthorization(&view, protocol.ConversationLoginState{Kind: "pending", Data: protocol.ConversationLoginStatePendingPayload{
			VerificationUrl: "https://accounts.example.test/activate", UserCode: "ABCD-EFGH", ExpiresAtMs: 100,
		}})
		if expiry != 100 || terminal || view.Flow != settings.FlowDevice || view.Code != "ABCD-EFGH" {
			t.Fatal("device flow facts were not projected")
		}
		projectAuthorization(&view, protocol.ConversationLoginState{Kind: "connected"})
		if view.Open {
			t.Fatal("connected authorization dialog stayed open")
		}
		ctx, cancel := context.WithCancel(t.Context())
		defer cancel()
		redirect, returned, manual, err := localAuthorization(ctx, callbackPresentation())
		if err != nil || redirect == nil || returned == nil || manual {
			t.Fatalf("controller callback listener: %v", err)
		}
		client := &http.Client{}
		response, err := client.Get(*redirect + "?code=captured&state=proof")
		if err != nil {
			t.Fatal(err)
		}
		response.Body.Close()
		if response.StatusCode != http.StatusNoContent {
			t.Fatalf("valid callback was rejected: %d", response.StatusCode)
		}
		select {
		case callback := <-returned:
			if callback != *redirect+"?code=captured&state=proof" {
				t.Fatalf("callback was not preserved: %s", callback)
			}
		case <-ctx.Done():
			t.Fatal(ctx.Err())
		}
		response, err = client.Get(*redirect + "?code=second&state=proof")
		if err != nil {
			t.Fatal(err)
		}
		response.Body.Close()
		if response.StatusCode != http.StatusConflict {
			t.Fatal("callback listener accepted a second result")
		}
	})
}
