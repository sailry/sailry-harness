package engine

import (
	"bytes"
	"context"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
)

func streamReadTransparent(name string) bool {
	return operations.IsUpdatePlanTool(name) || name == "set_session_title"
}

const maxConcurrentTools = 16

type streamedReads struct {
	ctx      context.Context
	end      func()
	slots    chan struct{}
	workers  sync.WaitGroup
	pending  map[*canonical.ToolRequest]*streamedRead
	skipped  map[*canonical.ToolRequest]canonical.ToolRequest
	accepted map[string]*streamedRead
	priorIDs map[string]bool
	// keys are the reads started or deferred so far; a repeat waits for
	// the batch, which answers it by reference (runToolBatch).
	keys    map[string]bool
	next    int
	blocked bool
}

func priorToolCallIDs(records []canonical.Record) map[string]bool {
	var seen map[string]bool
	for _, record := range records {
		if message := record.Message(); message != nil {
			for _, block := range message.Content {
				if call, ok := block.Data.(*canonical.ToolRequest); ok && call.ID != "" {
					if seen == nil {
						seen = map[string]bool{}
					}
					seen[call.ID] = true
				}
			}
		}
	}
	return seen
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
	if call.ID != "" && reads.priorIDs[call.ID] {
		// separateRepeatedCalls will rewrite this call ID once the stream ends,
		// so starting it speculatively under the stale ID would run it twice.
		reads.blocked = true
		return
	}
	if call.Error == nil && streamReadTransparent(call.Name) {
		return
	}
	tool := findTool(x.tools, call.Name)
	if call.Error != nil || tool == nil || !tool.ReadOnly || !tool.Concurrent || tool.Approval ||
		tool.Confirm != nil || tool.ObserveHistory != nil || tool.Delegation || x.hooks.WrapsTool(call.Name) {
		reads.blocked = true
		return
	}
	if validateToolArguments(tool, call).Failure != nil {
		reads.blocked = true
		return
	}
	if key := repeatKey(call); key != "" {
		if reads.keys[key] {
			// Record repeated concurrent reads as skipped so adopt() does not
			// break its prefix scan and cancel subsequent valid streamed reads.
			if reads.skipped == nil {
				reads.skipped = make(map[*canonical.ToolRequest]canonical.ToolRequest)
			}
			reads.skipped[request] = call
			return
		}
		if reads.keys == nil {
			reads.keys = map[string]bool{}
		}
		reads.keys[key] = true
	}
	// A read-only concurrent tool without StreamRead joins the same concurrent
	// batch as following reads and does not mutate state they observe, so it
	// need not block later native reads from starting during the stream.
	if !tool.StreamRead {
		if reads.skipped == nil {
			reads.skipped = make(map[*canonical.ToolRequest]canonical.ToolRequest)
		}
		reads.skipped[request] = call
		return
	}
	if reads.ctx == nil {
		reads.ctx, reads.end = r.beginToolStep(x)
		reads.slots = make(chan struct{}, maxConcurrentTools)
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
		select {
		case reads.slots <- struct{}{}:
			defer func() { <-reads.slots }()
		case <-ctx.Done():
			return
		}
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
		reads.skipped = nil
		return false
	}
	reads.accepted = make(map[string]*streamedRead, len(reads.pending))
	for _, block := range blocks {
		request, ok := block.Data.(*canonical.ToolRequest)
		if !ok || externalToolCall(*request) {
			continue
		}
		if request.Error == nil && streamReadTransparent(request.Name) {
			continue
		}
		if skipped, ok := reads.skipped[request]; ok {
			if request.Error != nil || request.ID != skipped.ID || request.Name != skipped.Name || !bytes.Equal(request.Arguments, skipped.Arguments) {
				break
			}
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
	reads.pending, reads.skipped = nil, nil
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
		reads.end, reads.pending, reads.skipped, reads.accepted = nil, nil, nil, nil
	}
}
