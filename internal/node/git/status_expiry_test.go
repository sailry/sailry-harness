package git

import (
	"context"
	"testing"
	"testing/synctest"
	"time"
)

func TestIdleStatusExpiryDoesNotEvictReplacement(t *testing.T) {
	t.Parallel()
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
		s.mu.Lock()
		replacement := s.statuses[root]
		s.mu.Unlock()
		if replacement == nil {
			t.Fatal("status was not cached")
		}
		time.Sleep(500 * time.Millisecond)
		synctest.Wait()
		s.mu.Lock()
		current := s.statuses[root]
		s.mu.Unlock()
		if current != replacement {
			t.Fatal("old expiry evicted the replacement")
		}
		time.Sleep(500 * time.Millisecond)
		synctest.Wait()
		s.mu.Lock()
		remaining := len(s.statuses)
		s.mu.Unlock()
		if remaining != 0 {
			t.Fatal("idle service retained expired status")
		}
	})
}
