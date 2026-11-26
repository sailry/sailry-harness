package node

import (
	"encoding/json"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	filesnode "github.com/sailry/sailry-harness/modules/files/node"
)

func TestNativeFileToolsQueueTheSameCanonicalTarget(t *testing.T) {
	t.Parallel()
	root := t.TempDir()
	inv := &store.AgentInvocation{Worktree: protocol.Worktree{Path: root}, Turn: protocol.DefaultQueuedTurn()}
	tools, err := filesnode.Module().ToolsForTurn(t.Context(), nodeModuleHost{node: &Node{}}, inv)
	if err != nil {
		t.Fatal(err)
	}
	for name, arguments := range map[string]string{
		"read_file":  `{"path":"src/main.go"}`,
		"edit_file":  `{"path":"src/main.go","old_text":"old","new_text":"new"}`,
		"write_file": `{"path":"src/main.go","text":"new"}`,
	} {
		i := slices.IndexFunc(tools, func(tool engine.Tool) bool { return tool.Definition.Name == name })
		if i < 0 || tools[i].FilePaths == nil {
			t.Fatalf("%s is missing its file target", name)
		}
		got := tools[i].FilePaths(canonical.ToolRequest{Name: name, Arguments: json.RawMessage(arguments)})
		if want := []string{workspaceFileKey(root, "src/main.go")}; !slices.Equal(got, want) {
			t.Fatalf("%s resolved %v, want %v", name, got, want)
		}
	}
}
