//go:build integration

package capture

import (
	"context"
	"testing"
)

// answered sends the access request while access is already granted: the
// system answers without a prompt, which exercises the completion block on
// the system's queue.
func answered(t *testing.T, ctx context.Context) {
	t.Helper()
	if Status() != PermissionGranted {
		return
	}
	if granted, err := ask(ctx); err != nil || !granted {
		t.Fatalf("request with granted access = %v, %v", granted, err)
	}
}
