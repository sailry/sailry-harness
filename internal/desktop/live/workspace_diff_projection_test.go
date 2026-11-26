package live

import (
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
	"testing"
)

func TestCheckpointDiffMetadata(t *testing.T) {
	const path = "fixture.bin"
	turn := workspaceTurnFiles(protocol.ConversationCheckpointTurnDiff{Files: []protocol.GitDiff{{Path: path, Binary: true, Truncated: true}}})
	if turn[0].Diff != (viewmodel.DiffState{Scope: "all", Binary: true, Partial: true}) {
		t.Fatalf("turn diff: %+v", turn[0].Diff)
	}
}
