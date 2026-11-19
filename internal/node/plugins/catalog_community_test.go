package plugins

import (
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCommunityPluginCatalogPages(t *testing.T) {
	plugins := []map[string]any{
		{"slug": "root", "name": "Root plugin", "version": "1.2.3", "repository": "https://github.com/acme/root", "manifestPath": "plugin.json"},
		{"slug": "nested", "name": "Nested plugin", "description": "Review projects", "keywords": []string{"code-quality"}, "repo": "acme/monorepo", "repository": "https://github.com/other/project", "manifestPath": "plugins/nested/plugin.json"},
		{"slug": "unsafe-repository", "name": "Unsafe repository", "repository": "https://github.com.example/acme/root", "manifestPath": "plugin.json"},
		{"slug": "unsafe-path", "name": "Unsafe path", "repo": "acme/root", "manifestPath": "plugins/../plugin.json"},
	}
	for i := range 49 {
		name := fmt.Sprintf("entry-%02d", i)
		plugins = append(plugins, map[string]any{"slug": name, "name": name, "repo": "acme/fixtures", "manifestPath": "plugins/" + name + "/plugin.json"})
	}
	var requests atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requests.Add(1)
		if r.URL.RequestURI() != "/api/v1/registry-manifest.json" {
			t.Errorf("unexpected community feed request: %s", r.URL.RequestURI())
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"plugins": plugins})
	}))
	defer server.Close()
	manager := NewManager(t.TempDir())
	manager.ThirdPartyURL = server.URL + "/"
	source := protocol.PluginCatalogSourceThirdParty
	ctx := t.Context()
	first, err := manager.Catalog(ctx, source, "", 1, false)
	if err != nil || first.Page != 1 || first.Pages != 2 || len(first.Entries) != 48 || first.Entries[0].ID != "community:root" || first.Entries[1].ID != "community:nested" {
		t.Fatalf("first page: page %d of %d, %d entries, %v", first.Page, first.Pages, len(first.Entries), err)
	}
	if root := first.Entries[0]; root.Version == nil || *root.Version != "1.2.3" || root.Bundled {
		t.Fatalf("community version or ownership: %+v", root)
	}
	last, err := manager.Catalog(ctx, source, "", 2, true)
	if err != nil || last.Page != 2 || last.Pages != 2 || len(last.Entries) != 3 || last.Entries[0].ID != "community:entry-46" {
		t.Fatalf("last page: %+v, %v", last, err)
	}
	for _, query := range []string{" CODE-QUALITY ", "review projects"} {
		matches, err := manager.Catalog(ctx, source, query, 1, false)
		if err != nil || matches.Pages != 1 || len(matches.Entries) != 1 || matches.Entries[0].ID != "community:nested" {
			t.Fatalf("query %q: %+v, %v", query, matches, err)
		}
	}
	for _, want := range []struct{ id, repository, folder string }{
		{"root", "https://github.com/acme/root", ""},
		{"community:nested", "https://github.com/acme/monorepo", "plugins/nested"},
	} {
		resolved, err := manager.CatalogSource(ctx, source, want.id)
		if err != nil || resolved.Repository != want.repository || resolved.Path == nil || *resolved.Path != want.folder {
			t.Fatalf("source for %s: %+v, %v", want.id, resolved, err)
		}
	}
	var fault *protocol.Fault
	if _, err := manager.CatalogSource(ctx, source, "community:NESTED"); !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeNotFound {
		t.Fatalf("an inexact slug resolved: %v", err)
	}
	if got := requests.Load(); got != 1 {
		t.Fatalf("paging, searching and inspecting redownloaded the manifest %d times", got)
	}
	if _, err := manager.Catalog(ctx, source, "", 1, true); err != nil || requests.Load() != 2 {
		t.Fatalf("explicit first-page refresh: %d requests, %v", requests.Load(), err)
	}
}

func TestCommunityPluginCatalogUnavailable(t *testing.T) {
	for _, tt := range []struct {
		name   string
		status int
		body   string
		fault  protocol.ErrorCode
	}{
		{"upstream unavailable", http.StatusPaymentRequired, `{"error":"deployment disabled"}`, protocol.ErrorCodeUnavailable},
		{"unexpected envelope", http.StatusOK, `{"data":[]}`, protocol.ErrorCodeInvalidRequest},
	} {
		t.Run(tt.name, func(t *testing.T) {
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				w.WriteHeader(tt.status)
				_, _ = fmt.Fprint(w, tt.body)
			}))
			defer server.Close()
			manager := NewManager(t.TempDir())
			manager.ThirdPartyURL = server.URL
			_, err := manager.Catalog(t.Context(), protocol.PluginCatalogSourceThirdParty, "", 1, false)
			var fault *protocol.Fault
			if !errors.As(err, &fault) || fault.Code != tt.fault {
				t.Fatalf("catalog fault: %v, want %s", err, tt.fault)
			}
		})
	}
}
