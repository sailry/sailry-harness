// Package process owns execution-Node command lifetime and bounded output.
package process

import (
	"context"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type Launch struct {
	ID             protocol.RequestID
	Session        protocol.SessionID
	Turn           protocol.TurnID
	Worktree       protocol.WorktreeID
	Root           string
	Profile        string
	Command        string
	Access         protocol.ProcessAccess
	Cwd            string
	TimeoutMs      uint64
	Pty            bool
	Background     bool
	Scratch        string
	Inputs         string
	Environment    map[string]string
	Release        func()
	CompletionGate <-chan struct{}
}

// Finished is persisted by the existing Node writer before completion publishes.
type Finished struct {
	Snapshot  protocol.ProcessSnapshot
	ElapsedMs *uint64
	Explicit  bool
}

type Options struct {
	Context        context.Context
	Profile        string
	Environment    Environment
	Now            func() time.Time
	After          func(time.Duration) <-chan time.Time
	Finished       func(context.Context, Finished) error
	Changed        func(protocol.WorktreeID)
	CaptureManaged func(string) (string, error)
	// Working, when set, brackets every running command.
	Working func() func()
	Run     func(context.Context, Launch, func(protocol.ProcessOutputOutputStream, []byte), <-chan []byte) (protocol.ProcessCompletion, error)
}
