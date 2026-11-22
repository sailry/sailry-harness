package codeindex

import (
	"context"
	"encoding/hex"

	"github.com/sailry/sailry-harness/internal/node/codeindexapi"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

// Fingerprint identifies this engine's extraction rules in files.fingerprint.
// Records written under any other rules are parsed again.
var Fingerprint = Hash("code-index-v2:gotreesitter:v0.57.0:embedded-1:source-1:references-2:budget-64m")

// Watch subscribes to changes of one worktree root. Its owner delivers every
// batch to Observed (empty for unknown changes) before waking the subscription.
// Successful wakes may coalesce; subscription errors report loss of the watch.
type Watch = codeindexapi.Watch

// Hash is the content revision of a source.
func Hash(source string) string {
	return hashBytes([]byte(source))
}

func hashBytes(data []byte) string {
	sum := blake3.Sum256(data)
	return hex.EncodeToString(sum[:])
}

// ReaderWaitExpired identifies only the live scan budget exhausted waiting for
// a file reader, not the operation deadline or another busy fault.
func ReaderWaitExpired(f *protocol.Fault) bool {
	return codeindexapi.ReaderWaitExpired(f)
}

func (s *Service) Outline(ctx context.Context, id protocol.WorktreeID, root, path string) (protocol.CodeIndexOutline, error) {
	return s.OutlineOnDemand(ctx, id, root, path)
}
