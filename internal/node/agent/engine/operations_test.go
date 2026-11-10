package engine

import (
	"context"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestOperationDrainSealsTurn(t *testing.T) {
	turn := protocol.NewTurnID()
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	owner := &execution{ctx: ctx}
	runtime := &Runtime{active: map[protocol.TurnID]*execution{turn: owner}}
	operation, release, ok := runtime.BeginOperation(turn)
	if !ok || operation != ctx {
		t.Fatal("running turn did not admit the operation")
	}
	drained := make(chan struct{})
	go func() { runtime.drainOperations(owner); close(drained) }()
	select {
	case <-drained:
		t.Fatal("turn settled with an owned effect")
	default:
	}
	release()
	release()
	<-drained
	if _, _, ok := runtime.BeginOperation(turn); ok {
		t.Fatal("finishing turn admitted a late effect")
	}
	other := protocol.NewTurnID()
	stopped, stop := context.WithCancel(context.Background())
	stop()
	runtime.active[other] = &execution{ctx: stopped}
	if _, _, ok := runtime.BeginOperation(other); ok {
		t.Fatal("cancelled turn admitted an effect")
	}
}
