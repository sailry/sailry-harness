package turso

import (
	"context"
	"runtime"
	"sync/atomic"
	"testing"
	"time"

	sdk "turso.tech/database/tursogo"
)

func TestOperationFastPathAndCancelInterrupt(t *testing.T) {
	t.Run("non-cancelled context does not interrupt or spawn ticker", func(t *testing.T) {
		ctx, cancel := context.WithCancel(context.Background())
		defer cancel()

		var conn sdk.TursoConnection
		var interrupts atomic.Int32
		before := runtime.NumGoroutine()
		for range 32 {
			op := newOperationWithInterrupt(ctx, conn, func(sdk.TursoConnection) {
				interrupts.Add(1)
			})
			if err := op.enter(); err != nil {
				t.Fatalf("enter: %v", err)
			}
			op.leave()
			op.close()
		}
		if interrupts.Load() != 0 {
			t.Fatalf("interrupts = %d, want 0", interrupts.Load())
		}
		if after := runtime.NumGoroutine(); after > before+2 {
			t.Fatalf("goroutines grew from %d to %d", before, after)
		}
	})

	t.Run("cancelled active operation interrupts and stops on close", func(t *testing.T) {
		ctx, cancel := context.WithCancel(context.Background())
		var conn sdk.TursoConnection
		var interrupts atomic.Int32
		first := make(chan struct{})
		op := newOperationWithInterrupt(ctx, conn, func(sdk.TursoConnection) {
			if interrupts.Add(1) == 1 {
				close(first)
			}
		})
		if err := op.enter(); err != nil {
			t.Fatalf("enter: %v", err)
		}
		cancel()
		select {
		case <-first:
		case <-time.After(time.Second):
			t.Fatal("cancelled operation did not interrupt within 1s")
		}
		op.leave()
		op.close()
		op.close()
		afterClose := interrupts.Load()
		time.Sleep(15 * time.Millisecond)
		if got := interrupts.Load(); got != afterClose {
			t.Fatalf("interrupts continued after close: before=%d after=%d", afterClose, got)
		}
	})
}
