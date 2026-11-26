package engine

import (
	"context"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type toolAccess struct {
	parallel          bool
	read              bool
	needsReadRevision bool
	paths             []string
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
		access := toolAccess{parallel: len(paths) > 0, paths: paths, read: tool.ReadOnly}
		if !tool.ReadOnly && tool.ObserveHistory != nil && op != protocol.ToolOperationWriteFile {
			var args struct {
				Expected json.RawMessage `json:"expected_revision"`
			}
			access.needsReadRevision = json.Unmarshal(call.Arguments, &args) == nil && len(args.Expected) == 0
		}
		return access
	}
	switch op {
	case protocol.ToolOperationFindFiles, protocol.ToolOperationListDirectory, protocol.ToolOperationSearchFiles:
		return toolAccess{parallel: tool.ReadOnly}
	case protocol.ToolOperationRunCommand:
		var args struct {
			Background bool `json:"background"`
		}
		return toolAccess{parallel: json.Unmarshal(call.Arguments, &args) == nil && !args.Background}
	default:
		return toolAccess{}
	}
}

// Writers follow earlier writes to the same files and, when they need a
// revision, earlier reads of those files. Admission stays on the caller
// goroutine so only one approval is pending. All launched effects drain.
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
	after := func(i, prev int) {
		next[prev] = append(next[prev], i)
		pending[i]++
	}
	for i, call := range access {
		for _, file := range call.paths {
			if call.read {
				readers[file] = append(readers[file], i)
				continue
			}
			if prev, ok := last[file]; ok && prev != i {
				after(i, prev)
			}
			if call.needsReadRevision {
				for _, read := range readers[file] {
					after(i, read)
				}
				delete(readers, file)
			}
			last[file] = i
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
