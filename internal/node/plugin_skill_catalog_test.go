package node

import (
	"context"
	"io"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestOfficialSkillCommandsUseNodeProduct(t *testing.T) {
	t.Parallel()
	const name = "skill-command-fixture"
	commit, snapshot := strings.Repeat("b", 40), strings.Repeat("a", 40)
	feed := `{"version":1,"source_commit":"` + snapshot + `","packages":[{"id":"` + name + `","manifest":{"$schema":"` + plugins.ManifestSchema + `","name":"` + name + `","version":"1.0.0"},"versions":[{"version":"1.0.0","commit":"` + commit + `"}],"version_requirements":{"1.0.0":{"products":["code"]}}}],"skills":[{"package":"` + name + `","path":"skills/inspect/SKILL.md","frontmatter":"---\nname: inspect\ndescription: Inspect changes\n---\n"}]}`
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, _ *http.Request) {
		_, _ = io.WriteString(writer, feed)
	}))
	t.Cleanup(server.Close)
	owner, err := StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), Options{ProductID: "harness", MCP: fixtureMCPOptions()})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := owner.Stop(context.Background()); err != nil {
			t.Error(err)
		}
	})
	owner.plugins.manager.OfficialURL = server.URL
	page, ok := dispatchPayload[protocol.PluginSkillcatalogPage](commandFixture(t, owner, protocol.Command{Kind: "search_skill_catalog", Data: protocol.CommandSearchSkillCatalogPayload{
		Source: protocol.PluginSkillcatalogSourceOfficial, Query: "inspect", Page: 1,
	}}).Data)
	if !ok || page.Unavailable || len(page.Entries) != 0 {
		t.Fatalf("Harness official search offered a Code-only skill: %+v", page)
	}
	path := name + "/skills/inspect"
	source := protocol.PluginSkillsResolved{Repository: plugins.OfficialRepository + ".git", GitRef: commit, Commit: commit}
	for _, command := range []protocol.Command{
		{Kind: "discover_skills", Data: protocol.CommandDiscoverSkillsPayload{Source: protocol.PluginSkillsSource{Repository: source.Repository, GitRef: &commit, Path: &path}}},
		{Kind: "read_skill_document", Data: protocol.CommandReadSkillDocumentPayload{Source: protocol.PluginSkillsDocumentSource{Kind: "git", Data: protocol.PluginSkillsDocumentSourceGitPayload{Source: source, Path: path}}}},
		{Kind: "install_skill", Data: protocol.CommandInstallSkillPayload{Source: source, Path: path, Name: plugins.StandaloneSkillName(source.Repository, path)}},
	} {
		result := dispatchFixture(t, owner, owner.ID(), protocol.NewRequest(owner.ID(), command))
		if result.Fault == nil || !strings.Contains(result.Fault.Message, "plugin does not support product harness") {
			t.Errorf("%s bypassed the official parent package requirements: %+v", command.Kind, result)
		}
	}
}
