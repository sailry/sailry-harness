package git

import (
	"context"
	"testing"
	"testing/synctest"
	"time"
)

func TestIdleStatusExpiryDoesNotEvictReplacement(t *testing.T) {
	root := t.TempDir()
	synctest.Test(t, func(t *testing.T) {
		s := New(Options{})
		read := func() {
			t.Helper()
			if _, err := s.Status(context.Background(), root); err != nil {
				t.Fatal(err)
			}
		}
		read()
		time.Sleep(500 * time.Millisecond)
		s.Invalidate(root)
		read()
		replacement := s.statuses[root]
		if replacement == nil {
			t.Fatal("status was not cached")
		}
		time.Sleep(500 * time.Millisecond)
		synctest.Wait()
		if s.statuses[root] != replacement {
			t.Fatal("old expiry evicted the replacement")
		}
		time.Sleep(500 * time.Millisecond)
		synctest.Wait()
		if len(s.statuses) != 0 {
			t.Fatal("idle service retained expired status")
		}
	})
}
