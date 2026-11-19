package live

import (
	"strings"
	"testing"
	"testing/synctest"

	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestSkillDetailsRetainSourceErrors(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, node := terminalTabsFixture(t)
		const id = "example/skills/office"
		r.App.Settings.MarketSkills = []viewmodel.Skill{{ID: id}}
		entry := protocol.DefaultPluginSkillcatalogEntry()
		entry.ID, entry.Name = id, "office"
		entry.Source = &protocol.PluginSkillcatalogInstallSource{Kind: "git", Data: protocol.PluginSkillsSource{Repository: "https://skills.sh/" + id}}
		r.packages.skillCatalog[id] = entry
		const discoverFailure = "skill discovery was unavailable"
		const documentFailure = "skill document was unavailable"
		failDiscovery, failDocument := true, true
		node.respond = func(request protocol.Request) link.Response {
			switch request.Command.Kind {
			case "discover_skills":
				if failDiscovery {
					failDiscovery = false
					return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: discoverFailure}}
				}
				value := protocol.DefaultPluginSkillsDiscovery()
				value.Source = protocol.PluginSkillsResolved{Repository: "https://github.com/example/skills", GitRef: "HEAD", Commit: strings.Repeat("a", 40)}
				candidate := protocol.DefaultPluginSkillsCandidate()
				candidate.Name, candidate.Path, candidate.Skill.Name = "skill-office", "office", "office"
				value.Skills = append(value.Skills, candidate)
				return link.Response{Output: protocol.Output{Kind: "skill_discovery", Data: value}}
			case "read_skill_document":
				if failDocument {
					failDocument = false
					return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: documentFailure}}
				}
				doc := protocol.DefaultPluginSkillsDocument()
				doc.Skill.Name = "office"
				doc.Content = "---\nname: office\n---\nThe skill document"
				return link.Response{Output: protocol.Output{Kind: "skill_document", Data: doc}}
			default:
				t.Errorf("unexpected skill request %q", request.Command.Kind)
				return link.Response{}
			}
		}
		tt := pluginUpdateQueueTester(r)
		tt.SetLocale("en")
		for _, want := range [][2]string{{"market_skill_document_failed", discoverFailure}, {"market_skill_document_failed", documentFailure}, {}} {
			message := [2]string{"not completed"}
			r.loadSkillDocument(id, func(failure, reason string) { message = [2]string{failure, reason} })
			for range 2 {
				synctest.Wait()
				tt.Frame()
			}
			if message != want || tt.HasText("Plugin operation failed") {
				t.Fatalf("details completed with %q, want %q, texts %q", message, want, tt.Texts())
			}
		}
		if got := r.App.Settings.MarketSkills[0].Content; got != "The skill document" {
			t.Fatalf("the retried document body was %q", got)
		}
	})
}
