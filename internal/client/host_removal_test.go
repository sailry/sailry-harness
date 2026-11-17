package client

import (
	"context"
	"errors"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
)

func TestHostRemovalLocalSaveIgnoresCallerCancellation(t *testing.T) {
	for _, failure := range []error{nil, errors.New("pairing storage failed")} {
		ctx, cancel := context.WithCancel(t.Context())
		cancel()
		calls := 0
		_, err := RemoveHost(ctx, &link.Link{}, link.EndpointAddress{}, func(local context.Context) error {
			calls++
			if local.Err() != nil || local.Done() != nil {
				t.Fatal("caller cancellation reached the local removal")
			}
			return failure
		})
		if calls != 1 || !errors.Is(err, failure) {
			t.Fatalf("local removal = %d calls, %v; want one call, %v", calls, err, failure)
		}
	}
}
