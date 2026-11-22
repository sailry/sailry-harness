package codeindex

import "github.com/sailry/sailry-harness/internal/protocol"

type (
	Range        = protocol.CodeIndexRange
	Symbol       = protocol.CodeIndexSymbol
	Status       = protocol.CodeIndexStatus
	Capabilities = protocol.CodeIndexCapabilities
)

// Parsed is the indexable content of one file. Ranges keep UTF-8 coordinates.
type Parsed struct {
	Path         string
	Language     string
	Hash         string
	Status       Status
	Symbols      []Symbol
	References   []Usage
	Imports      []string
	Capabilities Capabilities
	Partial      bool
	Truncated    bool
}

// Usage is one reference occurrence. Bytes and columns are UTF-8 offsets.
type Usage struct {
	Name   string
	Kind   string
	Line   uint32
	Column uint32
	Start  uint32
	// Receiver marks a name written through self or this.
	Receiver bool
	// Parsed references in the same declaration share a read-only Scope.
	Scope *Scope
}

// Scope is the innermost indexed declaration containing a reference.
type Scope struct {
	Name      string
	Kind      string
	StartLine uint32
	EndLine   uint32
}

const (
	StatusParsed      = protocol.CodeIndexStatusParsed
	StatusUnsupported = protocol.CodeIndexStatusUnsupported
	StatusTooLarge    = protocol.CodeIndexStatusTooLarge
	StatusInvalid     = protocol.CodeIndexStatusInvalid
	StatusFailed      = protocol.CodeIndexStatusFailed
)

func omitted(file string, status Status) Parsed {
	return Parsed{Path: file, Status: status, Partial: status != StatusUnsupported, Truncated: status == StatusTooLarge}
}

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
