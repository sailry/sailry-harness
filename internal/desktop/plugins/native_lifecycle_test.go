package plugins

import (
	"context"
	"testing"
)

func testNativeRetainedLifecycle(t *testing.T) {
	t.Run("disconnect cancels services and releases final native resources once", func(t *testing.T) {
		native := NewNativeBinding(context.Background())
		released := 0
		native.Release = func() {
			if native.Context.Err() == nil {
				t.Error("native release preceded service cancellation")
			}
			released++
		}
		owner := NewHost(HostBinding{Context: t.Context(), Native: &native})
		if err := owner.Disconnect(); err != nil {
			t.Fatal(err)
		}
		if native.Context.Err() == nil || !native.Suspended() || released != 0 {
			t.Fatalf("disconnect did not retain the native owner: suspended=%t released=%d", native.Suspended(), released)
		}
		for range 2 {
			if err := owner.Close(); err != nil {
				t.Fatal(err)
			}
		}
		if native.Suspended() || released != 1 {
			t.Fatalf("final native release changed: suspended=%t released=%d", native.Suspended(), released)
		}
	})
	t.Run("final close releases without a preceding disconnect", func(t *testing.T) {
		native := NewNativeBinding(t.Context())
		released := 0
		native.Release = func() { released++ }
		owner := NewHost(HostBinding{Context: t.Context(), Native: &native})
		for range 2 {
			if err := owner.Close(); err != nil {
				t.Fatal(err)
			}
		}
		if released != 1 {
			t.Fatalf("native owner released %d times", released)
		}
	})
}
