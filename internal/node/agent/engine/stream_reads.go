package engine

import (
	"bytes"
	"context"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
)

type streamedReads struct {
	ctx      context.Context
	end      func()
	workers  sync.WaitGroup
	pending  map[*canonical.ToolRequest]*streamedRead
	accepted map[string]*streamedRead
	next     int
	blocked  bool
}

type streamedRead struct {
	call   canonical.ToolRequest
	cancel context.CancelFunc
	done   chan struct{}
	result ToolResult
	ran    bool

	// The worker can enter or leave execution before its call commits.
	activityMu    sync.Mutex
	running       bool
	startActivity func() func()
	stopActivity  func()
}

func (job *streamedRead) publishActivity(start func() func()) {
	job.activityMu.Lock()
	defer job.activityMu.Unlock()
	job.startActivity = start
	if job.running {
		job.stopActivity = start()
	}
}

func (r *Runtime) startStreamRead(x *execution, reads *streamedReads, request *canonical.ToolRequest) {
	if externalToolCall(*request) {
		return
	}
	call := *request
	x.names.restoreCall(&call)
	tool := findTool(x.tools, call.Name)
	if call.Error != nil || tool == nil || !tool.StreamRead || !tool.ReadOnly || !tool.Concurrent || tool.Approval ||
		tool.Confirm != nil || tool.ObserveHistory != nil || tool.Delegation || x.hooks.WrapsTools() {
		reads.blocked = true
		return
	}
	if validateToolArguments(tool, call).Failure != nil {
		reads.blocked = true
		return
	}
	if reads.ctx == nil {
		reads.ctx, reads.end = r.beginToolStep(x)
		reads.pending = make(map[*canonical.ToolRequest]*streamedRead)
	}
	ctx, cancel := context.WithCancel(reads.ctx)
	job := &streamedRead{call: call, cancel: cancel, done: make(chan struct{})}
	reads.pending[request] = job
	reads.workers.Add(1)
	go func() {
		defer reads.workers.Done()
		defer close(job.done)
		defer cancel()
		metric, started := r.telemetry.Capture(), time.Now()
		defer func() { observeTool(metric, ctx, x.tools, call.Name, job.result, started) }()
		defer containToolPanic(x, call, &job.result)
		if ctx.Err() != nil {
			return
		}
		if tool.Prepare != nil {
			if tool.ReleasePrepared != nil {
				defer tool.ReleasePrepared(call)
			}
			if err := tool.Prepare(ctx, call); err != nil {
				job.result = toolFault(err)
				return
			}
		}
		if ctx.Err() != nil {
			return
		}
		defer x.timeline.tool(call, false)()
		job.ran = true
		job.activityMu.Lock()
		job.running = true
		if job.startActivity != nil {
			job.stopActivity = job.startActivity()
		}
		job.activityMu.Unlock()
		defer func() {
			job.activityMu.Lock()
			defer job.activityMu.Unlock()
			job.running = false
			if job.stopActivity != nil {
				job.stopActivity()
			}
		}()
		job.result = tool.Execute(ctx, call)
	}()
}

// A provider can complete calls out of order. Only the prefix before the
// first unresolved call can reuse reads that started during the stream.
func (reads *streamedReads) adopt(blocks []canonical.Block) bool {
	if len(reads.pending) == 0 {
		return false
	}
	reads.accepted = make(map[string]*streamedRead, len(reads.pending))
	for _, block := range blocks {
		request, ok := block.Data.(*canonical.ToolRequest)
		if !ok || externalToolCall(*request) {
			continue
		}
		job := reads.pending[request]
		if job == nil || request.Error != nil || request.ID != job.call.ID || request.Name != job.call.Name || !bytes.Equal(request.Arguments, job.call.Arguments) {
			break
		}
		reads.accepted[request.ID] = job
	}
	for request, job := range reads.pending {
		if reads.accepted[request.ID] != job {
			job.cancel()
		}
	}
	for request, job := range reads.pending {
		if reads.accepted[request.ID] != job {
			<-job.done
		}
	}
	reads.pending = nil
	return len(reads.accepted) > 0
}

func (reads *streamedReads) read(id string) *streamedRead {
	if reads == nil {
		return nil
	}
	return reads.accepted[id]
}

func (reads *streamedReads) close() {
	if reads != nil && reads.end != nil {
		reads.end()
		reads.workers.Wait()
		reads.end, reads.pending, reads.accepted = nil, nil, nil
	}
}
