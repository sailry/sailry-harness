package node

import (
	"context"

	"github.com/sailry/sailry-harness/internal/node"
)

type Node = node.Node

type Options = node.Options

func StartWithOptions(ctx context.Context, path string, options Options) (*Node, error) {
	return node.StartWithOptions(ctx, path, options)
}
