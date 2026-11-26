package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"strings"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	briefnode "github.com/sailry/sailry-harness/modules/brief/node"
)

func TestHarnessNativeBrief(t *testing.T) {
	for _, source := range []string{"native", "legacy"} {
		t.Run(source, func(t *testing.T) {
			t.Parallel()
			script := &agentToolScript{}
			owner := startHarnessModuleNode(t, nil, func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return script, nil })
			config := protocol.DefaultSessionConfig()
			config.Provider, config.Model = seedFixtureModelProvider(t, owner).ID, "fixture"
			config.Mode, config.Permission = protocol.WorkModePlan, protocol.PermissionAsk
			firstTool, firstPackage := "write_brief", briefnode.ArtifactReference()
			if source == "legacy" {
				firstPackage = plugins.PackageReference(installPluginFixture(t, owner, "brief"))
				firstTool = plugins.ToolAlias("brief", "write_brief")
			}

			artifact := func(raw string) protocol.ArtifactSummary {
				var result struct {
					Data protocol.OutputArtifactWrittenPayload
				}
				if err := json.Unmarshal([]byte(raw), &result); err != nil {
					t.Errorf("decode artifact result %q: %v", raw, err)
				}
				return result.Data.Artifact
			}
			const sourceHTML = `<p onclick="ignore()">Original</p><script>ignore()</script>`
			var first protocol.ArtifactSummary
			script.reset(func(step int, results map[string]string) [][3]string {
				switch step {
				case 1:
					return [][3]string{
						{"discover", "tool_search", `{"query":"select:` + firstTool + `,write_brief,read_brief,list_briefs"}`},
						{"skill", "load_skill", `{"skill":"brief:brief"}`},
					}
				case 2:
					return [][3]string{{"create", firstTool, fmt.Sprintf(`{"title":"Original", "html":%q}`, sourceHTML)}}
				case 3:
					first = artifact(results["create"])
					return [][3]string{{"read", "read_brief", fmt.Sprintf(`{"id":%q}`, first.ID.String())}}
				case 4:
					return [][3]string{{"revise", "write_brief", fmt.Sprintf(`{"id":%q,"expected_revision":1,"title":"Revised","html":"<p>Updated</p>"}`, first.ID.String())}}
				case 5:
					return [][3]string{{"stale", "write_brief", fmt.Sprintf(`{"id":%q,"expected_revision":1,"title":"Stale","html":"<p>Stale</p>"}`, first.ID.String())}}
				case 6:
					return [][3]string{
						{"historical", "read_brief", fmt.Sprintf(`{"id":%q,"revision":1}`, first.ID.String())},
						{"list", "list_briefs", `{}`},
					}
				}
				return nil
			})
			start := protocol.DefaultConversationStart()
			start.Config, start.Message = &config, protocol.ConversationInput{Text: "Create and revise an HTML document"}
			session := runAndApproveSession(t, owner, start)
			script.mu.Lock()
			defer script.mu.Unlock()
			if first.Package != firstPackage || first.Revision != 1 || first.Session != session || first.Kind != protocol.ArtifactKindDocument {
				t.Fatalf("created artifact = %+v; result %q", first, script.results["create"])
			}
			revised := artifact(script.results["revise"])
			if revised.ID != first.ID || revised.Package != briefnode.ArtifactReference() || revised.Revision != 2 || revised.CreatedAt != first.CreatedAt {
				t.Fatalf("revised artifact = %+v; result %q", revised, script.results["revise"])
			}
			if source == "native" && !strings.Contains(script.results["skill"], "## Create a session artifact") {
				t.Errorf("missing native skill: %s", script.results["skill"])
			}
			for _, call := range []string{"read", "historical"} {
				var result struct{ Data protocol.ArtifactContent }
				wantLatest := uint32(1)
				if call == "historical" {
					wantLatest = 2
				}
				if err := json.Unmarshal([]byte(script.results[call]), &result); err != nil || result.Data.Artifact != first || result.Data.LatestRevision != wantLatest || result.Data.Html != "<p>Original</p>" || result.Data.View != nil {
					t.Errorf("%s returned %q: %v", call, script.results[call], err)
				}
			}
			var listed struct{ Data protocol.ArtifactPage }
			if err := json.Unmarshal([]byte(script.results["list"]), &listed); err != nil || len(listed.Data.Items) != 1 || listed.Data.Items[0] != revised {
				t.Errorf("list returned %q: %v", script.results["list"], err)
			}
			if !strings.Contains(script.results["stale"], "artifact revision changed") {
				t.Errorf("stale write returned %q", script.results["stale"])
			}
			var history protocol.ConversationHistory
			if err := owner.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
				var err error
				history, err = store.ReadConversation(t.Context(), db, session, nil, 100)
				return err
			}); err != nil {
				t.Fatal(err)
			}
			if len(history.Page.Approvals) != 3 {
				t.Fatalf("expected three write approvals, got %+v", history.Page.Approvals)
			}
			for _, approval := range history.Page.Approvals {
				if approval.State != protocol.ConversationApprovalStateApproved || approval.Source != protocol.ConversationApprovalSourceStorage || approval.CanRemember {
					t.Errorf("Plan artifact write required user approval: %+v", approval)
				}
			}
		})
	}
}
