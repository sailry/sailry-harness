package engine

import (
	"context"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type toolAccess struct {
	parallel bool
	read     bool
	command  bool
	paths    []string
}

func parallelToolAccess(tool *Tool, call canonical.ToolRequest) toolAccess {
	if tool.ReadOnly && (!tool.Concurrent || tool.Confirm != nil || tool.Approval) || tool.Delegation {
		return toolAccess{}
	}
	var op protocol.ToolOperation
	if tool.Binding != nil && tool.Binding.Operation != nil && nativeTool(*tool) {
		op = *tool.Binding.Operation
	}
	if tool.FilePaths != nil {
		paths := tool.FilePaths(call)
		return toolAccess{parallel: len(paths) > 0, paths: paths, read: tool.ReadOnly}
	}
	switch op {
	case protocol.ToolOperationFindFiles, protocol.ToolOperationListDirectory, protocol.ToolOperationSearchFiles:
		return toolAccess{parallel: tool.ReadOnly}
	case protocol.ToolOperationRunCommand:
		var args struct {
			Background bool `json:"background"`
		}
		return toolAccess{parallel: json.Unmarshal(call.Arguments, &args) == nil && !args.Background, command: true}
	default:
		return toolAccess{}
	}
}

// Reads can overlap. Writes on the same file run in request order with both
// reads and writes. Admission stays on the caller goroutine.
func runQueuedTools(ctx context.Context, start int, access []toolAccess, admit func(int) error, execute, complete func(int)) error {
	if len(access) == 1 {
		if err := admit(start); err != nil {
			return err
		}
		execute(start)
		complete(start)
		return nil
	}

	last := make(map[string]int)
	pending := make([]int, len(access))
	next := make([][]int, len(access))
	var ready []int
	readers := make(map[string][]int)
	var commands []int
	after := func(i, prev int) {
		next[prev] = append(next[prev], i)
		pending[i]++
	}
	for i, call := range access {
		// A shell command may mutate arbitrary workspace files, so following
		// file reads/writes in the same batch wait for preceding commands to
		// finish before preparing, while independent commands still overlap.
		if len(call.paths) > 0 {
			for _, cmd := range commands {
				after(i, cmd)
			}
		}
		for _, file := range call.paths {
			if prev, ok := last[file]; ok && prev != i {
				after(i, prev)
			}
			if call.read {
				readers[file] = append(readers[file], i)
				continue
			}
			for _, read := range readers[file] {
				after(i, read)
			}
			delete(readers, file)
			last[file] = i
		}
		if call.command {
			commands = append(commands, i)
		}
		if pending[i] == 0 {
			ready = append(ready, i)
		}
	}
	done := make(chan int, min(len(access), maxConcurrentTools))
	running := 0
	var err error
	for len(ready) > 0 || running > 0 {
		if len(ready) > 0 && running < cap(done) && err == nil && ctx.Err() == nil {
			i := ready[0]
			ready = ready[1:]
			if err = admit(start + i); err != nil {
				continue
			}
			running++
			go func() {
				execute(start + i)
				done <- i
			}()
			continue
		}
		if running == 0 {
			break
		}
		i := <-done
		running--
		complete(start + i)
		for _, successor := range next[i] {
			pending[successor]--
			if pending[successor] == 0 {
				ready = append(ready, successor)
			}
		}
	}
	return err
}
