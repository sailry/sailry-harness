package node

import (
	"bufio"
	"context"
	"database/sql"
	"errors"
	"fmt"
	"os"
	"os/exec"
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

// TestProfileLockAfterKill runs itself as the owner of a profile and kills
// that process: the operating system must release the lock with it, and the
// lock file that stays behind must not stop the next owner.
func TestProfileLockAfterKill(t *testing.T) {
	const variable = "SAILRY_TEST_PROFILE_OWNER"
	if path := os.Getenv(variable); path != "" {
		if _, err := OpenProfile(context.Background(), path); err != nil {
			fmt.Println(err)
			os.Exit(1)
		}
		fmt.Println("owned")
		time.Sleep(time.Minute)
		os.Exit(0)
	}
	t.Parallel()
	ctx := context.Background()
	path := filepath.Join(t.TempDir(), "profile")
	owner := exec.Command(os.Args[0], "-test.run=^TestProfileLockAfterKill$")
	owner.Env = append(os.Environ(), variable+"="+path)
	output, err := owner.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	if err := owner.Start(); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		_ = owner.Process.Kill()
		_ = owner.Wait()
	})
	line, err := bufio.NewReader(output).ReadString('\n')
	if err != nil || strings.TrimSpace(line) != "owned" {
		t.Fatalf("owner process: %q %v", line, err)
	}
	var owned *core.ProfileInUseError
	if _, err := OpenProfile(ctx, path); !errors.As(err, &owned) {
		t.Fatalf("a second owner of a live profile: %v", err)
	}
	if err := owner.Process.Kill(); err != nil {
		t.Fatal(err)
	}
	_ = owner.Wait()
	if _, err := os.Stat(filepath.Join(path, "node.lock")); err != nil {
		t.Fatalf("lock file after the owner died: %v", err)
	}
	profile, err := OpenProfile(ctx, path)
	if err != nil {
		t.Fatalf("profile after its owner was killed: %v", err)
	}
	if err := profile.Close(); err != nil {
		t.Fatal(err)
	}
}
