package desktop

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
)

func TestProviderOAuthSaveIgnoresHiddenKeyHeaderDraft(t *testing.T) {
	m := testController()
	m.Live = true
	var saved *settings.ProviderDraft
	m.ProviderActions.Save = func(draft settings.ProviderDraft, done func(settings.ProviderSaveResult)) {
		saved = &draft
		done(settings.ProviderSaveResult{Failed: []string{"fixture-node"}})
	}
	e := &m.provider
	e.add(m)
	keyed := preview.Method{ID: "keyed", Name: "Key channel", Keyed: true, Routes: []preview.Route{{ID: "keyed/0", API: "responses", Endpoint: "https://example.test/v1"}}}
	e.configure(keyed)
	e.key, e.headers = "fixture-key", []header{{"", "invalid header draft"}}
	if problem := e.Check(); problem != "provider_headers_invalid" {
		t.Fatalf("visible API-key header was not checked: %q", problem)
	}
	e.back()
	e.configure(preview.Method{ID: "oauth", Name: "OAuth channel", Login: "fixture-login", Routes: []preview.Route{{ID: "oauth/0", API: "responses", Endpoint: "https://example.test/v1"}}})
	if problem := e.save(m); problem != "" {
		t.Fatalf("a hidden API-key draft prevented OAuth save: %s", problem)
	}
	if saved == nil || saved.Channel.Auth != preview.AuthOAuth || len(saved.Channel.Headers) != 0 {
		t.Fatalf("OAuth save retained API-key headers: %+v", saved)
	}
	e.configure(keyed)
	if problem := e.Check(); problem != "provider_headers_invalid" {
		t.Fatalf("returning to the API-key method lost its header validation: %q", problem)
	}
}
