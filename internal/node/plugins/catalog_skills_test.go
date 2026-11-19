package plugins

import (
	"io"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestExternalSkillCatalogPages(t *testing.T) {
	const skills = `{"skills":[{"source":"acme/skills","skillId":"pdf","name":"PDF"}],"total":9904,"hasMore":true}`
	const search = `{"skills":[{"source":"acme/skills","skillId":"pdf","name":"PDF"}],"count":1}`
	const hub = `{"code":0,"data":{"skills":[{"namespace":{"handle":"team"},"slug":"pdf","name":"PDF","version":"1.0.0"}],"total":50}}`
	for _, tt := range []struct {
		name, query string
		source      protocol.PluginSkillcatalogSource
		page        uint32
		uri, body   string
		id          string
		next        uint32
	}{
		{"leaderboard", "", protocol.PluginSkillcatalogSourceSkillsSh, 1, "/api/skills/all-time/0", skills, "acme/skills/pdf", 2},
		{"short query", " p ", protocol.PluginSkillcatalogSourceSkillsSh, 2, "/api/skills/all-time/1", skills, "acme/skills/pdf", 3},
		{"search", " pdf ", protocol.PluginSkillcatalogSourceSkillsSh, 1, "/api/search?limit=100&q=pdf", search, "acme/skills/pdf", 0},
		{"hub browse", "", protocol.PluginSkillcatalogSourceSkillhub, 1, "/api/skills?page=1&pageSize=24", hub, "team/pdf", 2},
		{"hub search", "pdf", protocol.PluginSkillcatalogSourceSkillhub, 2, "/api/skills?keyword=pdf&page=2&pageSize=24", hub, "team/pdf", 3},
	} {
		t.Run(tt.name, func(t *testing.T) {
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				if r.URL.RequestURI() != tt.uri {
					t.Errorf("request = %s; want %s", r.URL.RequestURI(), tt.uri)
				}
				_, _ = io.WriteString(w, tt.body)
			}))
			defer server.Close()
			manager := NewManager(t.TempDir())
			manager.SkillsDirectoryURL, manager.RegistryURL = server.URL, server.URL
			page, err := manager.SkillCatalog(t.Context(), tt.source, tt.query, tt.page, false)
			if err != nil || len(page.Entries) != 1 || page.Entries[0].ID != tt.id || page.Entries[0].Source == nil || page.Unavailable || page.Page != tt.page {
				t.Fatalf("unexpected page: %+v, %v", page, err)
			}
			if tt.next == 0 && page.Next != nil || tt.next != 0 && (page.Next == nil || *page.Next != tt.next) {
				t.Fatalf("unexpected continuation: %v", page.Next)
			}
		})
	}
}

func TestExternalSkillCatalogCardFields(t *testing.T) {
	const skills = `{"skills":[{"source":"acme/skills","skillId":"pdf","name":"PDF","installs":1200,"isOfficial":true}],"total":1,"hasMore":false}`
	const hub = `{"code":0,"data":{"skills":[{"iconUrl":"https://cdn.example/pdf.png","namespace":{"handle":"team","displayName":" Team "},"slug":"pdf","name":"PDF","downloads":42,"stars":7,"verified":true}],"total":1}}`
	for _, tt := range []struct {
		name, body, publisher, avatar string
		source                        protocol.PluginSkillcatalogSource
		installs, stars               *uint64
	}{
		{"skills.sh", skills, "acme", "", protocol.PluginSkillcatalogSourceSkillsSh, new(uint64(1200)), nil},
		{"skillhub", hub, "Team", "https://cdn.example/pdf.png", protocol.PluginSkillcatalogSourceSkillhub, new(uint64(42)), new(uint64(7))},
	} {
		t.Run(tt.name, func(t *testing.T) {
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) { _, _ = io.WriteString(w, tt.body) }))
			defer server.Close()
			manager := NewManager(t.TempDir())
			manager.SkillsDirectoryURL, manager.RegistryURL = server.URL, server.URL
			page, err := manager.SkillCatalog(t.Context(), tt.source, "", 1, false)
			if err != nil || len(page.Entries) != 1 {
				t.Fatalf("unexpected page: %+v, %v", page, err)
			}
			entry := page.Entries[0]
			// A substitute skills.sh directory gets no GitHub picture.
			avatar := ""
			if entry.Avatar != nil {
				avatar = *entry.Avatar
			}
			if entry.Publisher == nil || *entry.Publisher != tt.publisher || avatar != tt.avatar || !entry.Verified {
				t.Fatalf("unexpected identity: %+v", entry)
			}
			same := func(a, b *uint64) bool { return a == nil && b == nil || a != nil && b != nil && *a == *b }
			if !same(entry.Installs, tt.installs) || !same(entry.Stars, tt.stars) {
				t.Fatalf("unexpected counts: %v %v", entry.Installs, entry.Stars)
			}
		})
	}
}

func TestExternalSkillCatalogRejectsErrorEnvelope(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = io.WriteString(w, `{"message":"not a catalog"}`)
	}))
	defer server.Close()
	manager := NewManager(t.TempDir())
	manager.SkillsDirectoryURL, manager.RegistryURL = server.URL, server.URL
	for _, source := range []protocol.PluginSkillcatalogSource{protocol.PluginSkillcatalogSourceSkillsSh, protocol.PluginSkillcatalogSourceSkillhub} {
		if _, err := manager.SkillCatalog(t.Context(), source, "", 1, false); err == nil {
			t.Errorf("%s accepted an error envelope as an empty catalog", source)
		}
	}
}
