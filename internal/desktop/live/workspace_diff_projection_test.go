package live

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestWorkspaceDiffMetadataFollowsCurrentProjection(t *testing.T) {
	const path = "fixture.bin"
	p := &workspaceProject{
		panel: &panel.Project{},
		data:  &viewmodel.Workspace{Changes: []viewmodel.FileChange{{Path: path}}},
		diffs: map[string]bool{"staged/" + path: true, "unstaged/" + path: true},
		manualDiffs: map[string]workspaceGitDiff{
			"staged/" + path:   {diff: protocol.GitDiff{Binary: true, Size: 100}},
			"unstaged/" + path: {diff: protocol.GitDiff{Truncated: true}},
		},
	}
	p.readDiff(path, "staged")
	if got := p.data.Changes[0].Diff; got != (viewmodel.DiffState{Scope: "staged", Binary: true, Size: 100}) {
		t.Fatalf("staged cache: %+v", got)
	}
	p.readDiff(path, "unstaged")
	if got := p.data.Changes[0].Diff; got != (viewmodel.DiffState{Scope: "unstaged", Partial: true}) {
		t.Fatalf("unstaged cache: %+v", got)
	}
	turn := workspaceTurnFiles(protocol.ConversationCheckpointTurnDiff{Files: []protocol.GitDiff{{Path: path, Binary: true, Truncated: true}}})
	if turn[0].Diff != (viewmodel.DiffState{Scope: "all", Binary: true, Partial: true}) {
		t.Fatalf("turn diff: %+v", turn[0].Diff)
	}
}
