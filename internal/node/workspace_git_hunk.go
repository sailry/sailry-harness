package node

import (
	"context"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) applyManualGitHunk(ctx context.Context, root string, input protocol.CommandApplyGitHunkPayload) (protocol.Output, error) {
	if err := node.protectEntry(root, input.Path); err != nil {
		return protocol.Output{}, err
	}
	defer node.watches.changed(root, nil)
	result, err := node.git.ApplyHunk(ctx, root, input)
	return protocol.Output{Kind: "git_hunk_applied", Data: result}, err
}
