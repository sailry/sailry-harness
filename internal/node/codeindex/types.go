package codeindex

import "github.com/sailry/sailry-harness/internal/protocol"

type (
	Range        = protocol.CodeIndexRange
	Symbol       = protocol.CodeIndexSymbol
	Status       = protocol.CodeIndexStatus
	Capabilities = protocol.CodeIndexCapabilities
)

const (
	StatusParsed      = protocol.CodeIndexStatusParsed
	StatusUnsupported = protocol.CodeIndexStatusUnsupported
	StatusTooLarge    = protocol.CodeIndexStatusTooLarge
	StatusInvalid     = protocol.CodeIndexStatusInvalid
	StatusFailed      = protocol.CodeIndexStatusFailed
)

// MaxFileBytes is the largest source the index parses.
const MaxFileBytes = 1024 * 1024

// Query bounds shared with every engine of the protocol.
const (
	maxSearchSymbols  = 100
	maxOutlineSymbols = 256
	maxGraphItems     = 200
	maxGraphOffset    = 10_000
	maxGraphDepth     = 3
	maxGraphPaths     = 64
)
