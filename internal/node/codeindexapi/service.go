package codeindexapi

import (
	"context"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Watch subscribes to a worktree. The Node calls Observed before waking the subscription.
type Watch func(ctx context.Context, root string, worktree protocol.WorktreeID) (link.Subscription, error)

// Factory supplies a profile's code index. A nil service means indexing is unavailable.
// The service can subscribe to worktrees after the factory returns.
type Factory func(profile string, watch Watch) Service

type Service interface {
	Configure(enabled bool)
	Close()
	Observed(id protocol.WorktreeID, paths []string)
	Remove(id protocol.WorktreeID) error
	Search(ctx context.Context, id protocol.WorktreeID, root string, query protocol.CodeIndexSearch) (protocol.CodeIndexSearchPage, error)
	SearchOnDemand(ctx context.Context, id protocol.WorktreeID, root string, query protocol.CodeIndexSearch) (protocol.CodeIndexSearchPage, error)
	OutlineOnDemand(ctx context.Context, id protocol.WorktreeID, root, path string) (protocol.CodeIndexOutline, error)
	Graph(ctx context.Context, id protocol.WorktreeID, root string, query protocol.CodeIndexGraph) (protocol.CodeIndexGraphGraphPage, error)
	GraphOnDemand(ctx context.Context, id protocol.WorktreeID, root string, query protocol.CodeIndexGraph) (protocol.CodeIndexGraphGraphPage, error)
}

const ReaderWaitMessage = "code query timed out waiting for a reader"

func ReaderWaitExpired(f *protocol.Fault) bool {
	return f != nil && f.Code == protocol.ErrorCodeBusy && f.Details == nil && f.Message == ReaderWaitMessage
}
