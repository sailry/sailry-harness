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

// A run that was killed leaves its scratch behind: database copies of a
// backup, an unpacked restore, read-only staged inputs. The next start
// removes it before anything uses the profile.
func TestProfileLeftovers(t *testing.T) {
	t.Parallel()
	ctx := context.Background()
	path := filepath.Join(t.TempDir(), "profile")
	first, err := OpenProfile(ctx, path)
	if err != nil {
		t.Fatal(err)
	}
	path = first.Ownership.Path
	if err := first.Close(); err != nil {
		t.Fatal(err)
	}
	outside := filepath.Join(t.TempDir(), "outside")
	if err := os.WriteFile(outside, []byte("kept"), 0o600); err != nil {
		t.Fatal(err)
	}
	write := func(name string, mode os.FileMode) string {
		t.Helper()
		target := filepath.Join(path, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(target), 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(target, []byte("left behind"), mode); err != nil {
			t.Fatal(err)
		}
		return target
	}
	removed := []string{
		filepath.Dir(write("tmp/backup-1/node.sqlite", 0o600)),
		filepath.Dir(write("tmp/restore-1/storage/node.sqlite", 0o600)),
		write("tmp/command-output-1", 0o600),
		filepath.Dir(write("storage/.upgrade-1/storage/node.sqlite", 0o600)),
		filepath.Dir(write("storage/.schema-reference-1/reference.sqlite", 0o600)),
		filepath.Dir(filepath.Dir(write("storage/backups/.upgrade-1/storage/node.sqlite", 0o600))),
		filepath.Dir(write("plugins/packages/.staging-1/plugin.json", 0o600)),
		filepath.Dir(write("python/staging/environment-1/pyvenv.cfg", 0o600)),
	}
	// Staged inputs are read-only all the way down.
	staged := filepath.Dir(write("tmp/sailry-inputs-1/nested/input", 0o400))
	for _, directory := range []string{staged, filepath.Dir(staged)} {
		if err := os.Chmod(directory, 0o500); err != nil {
			t.Fatal(err)
		}
	}
	removed = append(removed, filepath.Dir(staged))
	// A link in tmp is removed; what it names is not.
	link := filepath.Join(path, "tmp", "link")
	if err := os.Symlink(outside, link); err != nil {
		t.Fatal(err)
	}
	removed = append(removed, link)
	kept := []string{
		// The process service kills the group this names, then removes it.
		write("tmp/command-group-1", 0o600),
		write("storage/backups/node-before-upgrade-1/node.sqlite", 0o600),
		write("storage/.upgraded", 0o600),
		write("plugins/packages/published/plugin.json", 0o600),
		write("python/environments/published/pyvenv.cfg", 0o600),
		filepath.Join(path, "storage", "node.sqlite"),
		filepath.Join(path, "link.key"),
		outside,
	}

	second, err := OpenProfile(ctx, path)
	if err != nil {
		t.Fatalf("start over leftovers: %v", err)
	}
	defer second.Close()
	for _, name := range removed {
		if _, err := os.Lstat(name); !errors.Is(err, os.ErrNotExist) {
			t.Errorf("%s survived the start: %v", name, err)
		}
	}
	for _, name := range kept {
		if _, err := os.Lstat(name); err != nil {
			t.Errorf("the start removed %s: %v", name, err)
		}
	}
	if second.Identity.ID() != first.Identity.ID() {
		t.Error("the profile changed its identity")
	}
}
