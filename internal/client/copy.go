package client

import "github.com/sailry/sailry-harness/internal/protocol"

// A presentation receives its own mutable copy of the shared projection.
func copyValue[T any](value T) T { return protocol.Clone(value) }
