package engine

import (
	"context"
	"sync/atomic"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestToolCleanupPanicSettlesAndReleasesAllResources(t *testing.T) {
	var firstClosed, secondClosed atomic.Int32
	first := objectTool("first", true, true, func(context.Context, canonical.ToolRequest) ToolResult {
		return ToolResult{}
	})
	first.Close = func() error {
		if firstClosed.Add(1) == 1 {
			panic("fixture cleanup panic")
		}
		return nil
	}
	second := objectTool("second", true, true, nil)
	second.Close = func() error { secondClosed.Add(1); return nil }
	model := &scriptedModel{scripts: []script{
		response(text("First answer"), finish()),
		response(text("Next answer"), finish()),
	}}
	fixture := newFixture(t, model, first, second)
	fixture.start()
	turn := fixture.submit("First request")
	run := fixture.waitRun(turn, protocol.ConversationStatusFailed)
	if run.Error == nil || run.Error.Code != protocol.ErrorCodeOutcomeUnknown || firstClosed.Load() != 1 || secondClosed.Load() != 1 {
		t.Fatalf("cleanup panic settlement = %+v, closes = %d/%d", run.Error, firstClosed.Load(), secondClosed.Load())
	}
	fixture.waitRun(fixture.submit("Next request"), protocol.ConversationStatusCompleted)
	if firstClosed.Load() != 2 || secondClosed.Load() != 2 || model.count() != 2 {
		t.Fatalf("later turn cleanup = %d/%d, requests = %d", firstClosed.Load(), secondClosed.Load(), model.count())
	}
}
