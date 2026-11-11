package node

import (
	"context"
	"database/sql"
	"errors"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/core"
)

func TestStopDeadline(t *testing.T) {
	t.Parallel()
	ctx := context.Background()

	t.Run("work that ignores cancellation is abandoned and the profile released", func(t *testing.T) {
		t.Parallel()
		path := filepath.Join(t.TempDir(), "profile")
		stuck, err := startFixtureNode(ctx, path)
		if err != nil {
			t.Fatal(err)
		}
		stuck.grace = 200 * time.Millisecond
		// A request that never returns: nothing cancels it.
		if err := stuck.begin(); err != nil {
			t.Fatal(err)
		}
		started := time.Now()
		err = stuck.Stop(ctx)
		if !errors.Is(err, core.ErrShutdownTimeout) || !strings.Contains(err.Error(), "requests in flight did not stop") {
			t.Fatalf("shutdown past its deadline: %v", err)
		}
		if elapsed := time.Since(started); elapsed > 5*time.Second {
			t.Fatalf("shutdown waited %v for work that ignores cancellation", elapsed)
		}
		if err := stuck.Stop(ctx); !errors.Is(err, core.ErrShutdownTimeout) {
			t.Fatalf("repeated stop: %v", err)
		}
		// The next launch owns the profile and reads the same database.
		next, err := startFixtureNode(ctx, path)
		if err != nil {
			t.Fatalf("start after an abandoned shutdown: %v", err)
		}
		if next.ID() != stuck.ID() {
			t.Fatal("the profile changed its identity")
		}
		if err := next.Stop(ctx); err != nil {
			t.Fatal(err)
		}
		// Let the abandoned sequence end so the test leaves nothing running.
		stuck.work.Done()
	})

	t.Run("a database in use stays open", func(t *testing.T) {
		t.Parallel()
		value, err := startFixtureNode(ctx, filepath.Join(t.TempDir(), "profile"))
		if err != nil {
			t.Fatal(err)
		}
		value.grace = 200 * time.Millisecond
		if err := value.begin(); err != nil {
			t.Fatal(err)
		}
		reading, release := make(chan struct{}), make(chan struct{})
		read := make(chan error, 1)
		go func() {
			read <- value.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
				close(reading)
				<-release
				var one int
				return db.QueryRowContext(ctx, "SELECT 1").Scan(&one)
			})
		}()
		<-reading
		if err := value.Stop(ctx); !errors.Is(err, core.ErrShutdownTimeout) {
			t.Fatalf("shutdown past its deadline: %v", err)
		}
		close(release)
		if err := <-read; err != nil {
			t.Fatalf("a read in progress lost its connection: %v", err)
		}
		value.work.Done()
	})
}
