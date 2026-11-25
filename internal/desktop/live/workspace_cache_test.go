package live

import (
	"testing"
	"testing/synctest"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestWorkspaceReleasesClosedContent(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		fixture := newPluginLocationTestFixture(t)
		fixture.root.busy, fixture.root.uncertain = map[string]bool{}, map[string]protocol.Request{}
		project := fixture.root.workspaceFor(fixture.main)
		const clean, draft, diff, closed = "clean.md", "draft.md", "diff.go", "closed.go"
		for _, path := range []string{clean, draft, diff, closed} {
			project.fileLoaded(path, protocol.FileContent{Path: path, Text: "old content", Revision: new("r1")})
		}
		project.panel.Files.Show(draft)
		project.panel.FileChanged(draft, "my draft")
		project.panel.Files.Show(clean)
		project.panel.Git.Show(diff)
		project.draftSource(draft, changedFile{content: protocol.FileContent{Text: "changed on disk", Revision: new("r2")}})
		project.documentMarkdown(closed)
		images := project.documentImages[closed]
		if images == nil {
			t.Fatal("fixture did not load document images")
		}

		project.releaseView()
		if project.documentImages[closed] != nil || images.ctx.Err() == nil {
			t.Fatal("switching away kept a closed document's image cache")
		}
		if _, exists := project.files[closed]; exists {
			t.Fatal("the closed path retained its file body")
		}
		if project.data.Files != nil {
			t.Fatal("opening an explicit document constructed a management tree")
		}
		if text, dirty := project.panel.FileDraft(draft); !dirty || text != "my draft" || project.files[draft].Revision == nil || project.changedFiles[draft].content.Text != "changed on disk" {
			t.Fatal("the open draft or its expected revision was lost")
		}
		if project.files[clean].Text != "old content" || project.files[diff].Text != "old content" {
			t.Fatal("the open clean editor or diff lost its content")
		}
		project.panel.Git.Show(":changes")
		project.pruneClosedContent()
		if _, exists := project.files[diff]; exists {
			t.Fatal("the combined view should not keep the working file body")
		}
		reads := make(chan string, 3)
		fixture.source.dispatch = func(request protocol.Request) (link.Admission, error) {
			input, ok := detail[protocol.CommandReadFilePayload](request.Command.Data)
			if !ok || input.Worktree != fixture.main.ID {
				t.Errorf("unexpected reload on reopening a file: %v", request.Command.Kind)
			}
			reads <- input.Path
			completion := make(chan link.Response, 1)
			completion <- link.Response{Output: protocol.Output{Kind: "file", Data: protocol.FileContent{Path: input.Path, Text: "new content", Revision: new("r2")}}}
			return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
		}
		window := closeTestWindow(fixture.root)
		project.turnView = new(protocol.TurnID{1})
		project.readDiff(diff, "all")
		synctest.Wait()
		window.Frame()
		if len(reads) != 0 {
			t.Fatal("the combined turn review read a working file it does not display")
		}
		project.panel.Git.Show(diff)
		project.readDiff(diff, "all")
		synctest.Wait()
		window.Frame()
		if len(reads) != 1 || <-reads != diff || workspaceString(project.files[diff].Revision) != "r2" {
			t.Fatal("the single-file turn review did not load its current revision")
		}
		project.panel.FileOpen(closed)
		synctest.Wait()
		window.Frame()
		if text, dirty := project.panel.FileDraft(closed); text != "new content" || dirty {
			t.Fatalf("reopen did not load the current content: %q, dirty %v", text, dirty)
		}
	})
}
