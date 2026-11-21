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
	"runtime"
	"strings"
	"sync"
	"sync/atomic"
	"syscall"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type shutdownScript struct {
	fantasy.LanguageModel
	started chan context.Context
	finish  chan struct{}
	calls   atomic.Int32
}

func (script *shutdownScript) Stream(ctx context.Context, _ fantasy.Call) (fantasy.StreamResponse, error) {
	script.calls.Add(1)
	return func(yield func(fantasy.StreamPart) bool) {
		if !yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "answer", Delta: "Retained output"}) {
			return
		}
		select {
		case script.started <- ctx:
		case <-ctx.Done():
			return
		}
		select {
		case <-ctx.Done():
		case <-script.finish:
			yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop})
		}
	}, nil
}

func startShutdownNode(t *testing.T, path string, script *shutdownScript) *Node {
	t.Helper()
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	value, err := StartWithOptions(t.Context(), path, Options{
		Environment: &environment, MCP: fixtureMCPOptions(),
		Model: func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return script, nil },
	})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := value.Stop(context.Background()); err != nil {
			t.Error(err)
		}
	})
	return value
}

func TestStopInterruptsTurnsBeforeCommandLanes(t *testing.T) {
	t.Parallel()
	path := filepath.Join(t.TempDir(), "profile")
	script := &shutdownScript{started: make(chan context.Context, 2), finish: make(chan struct{})}
	value := startShutdownNode(t, path, script)
	session := sessionFixture(t, value)
	submit := func(text string) protocol.QueuedTurn {
		return commandFixture(t, value, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{
			Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: text},
		}}).Data.(protocol.QueuedTurn)
	}
	first := submit("Running input")
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
	defer cancel()
	var running context.Context
	select {
	case running = <-script.started:
	case <-ctx.Done():
		t.Fatal("model did not start")
	}
	queued := submit("Keep this pending input")

	// A workspace effect holds the registration lock needed by the admitted
	// project command, including its cancelled outcome during shutdown.
	root := t.TempDir()
	release := sync.OnceFunc(value.locks.effect(root))
	defer release()
	draft := protocol.DefaultProjectsDraft()
	draft.Name, draft.Path, draft.Source = "Blocked registration", root, protocol.ProjectsSource{Kind: "local"}
	admitted, err := value.Dispatch(ctx, value.ID(), protocol.NewRequest(value.ID(), protocol.Command{Kind: "create_project", Data: draft}))
	if err != nil {
		t.Fatal(err)
	}
	stopping, stopWaiting := context.WithCancel(ctx)
	stopWaiting()
	if err := value.Stop(stopping); !errors.Is(err, context.Canceled) {
		t.Fatalf("stop waiter = %v", err)
	}
	if running.Err() == nil {
		t.Fatal("Node cancelled its dependencies while its model was still running behind a blocked command lane")
	}
	select {
	case <-admitted.Completion:
		t.Fatal("the command lane did not remain blocked")
	default:
	}
	release()
	if err := value.Stop(ctx); err != nil {
		t.Fatal(err)
	}

	next := startShutdownNode(t, path, script)
	if err := next.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		run, err := store.ReadAgentRun(ctx, db, first.ID)
		if err != nil {
			return err
		}
		if run.Status != protocol.ConversationStatusInterrupted {
			return fmt.Errorf("shutdown run = %s, want interrupted", run.Status)
		}
		queue, err := store.ReadQueue(ctx, db, session.ID)
		if err != nil {
			return err
		}
		if !queue.Paused || queue.PauseReason == nil || *queue.PauseReason != protocol.ConversationQueuePauseInterrupted || len(queue.Items) != 1 || queue.Items[0].Turn != queued.ID || !queue.Items[0].Ready || queue.Items[0].Preview != "Keep this pending input" {
			return fmt.Errorf("restart did not retain the held queue: %+v", queue)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	if calls := script.calls.Load(); calls != 1 {
		t.Fatalf("shutdown or recovery replayed a model request: %d calls", calls)
	}
}

func TestClientCloseLeavesAdmittedTurnRunning(t *testing.T) {
	t.Parallel()
	script := &shutdownScript{started: make(chan context.Context, 2), finish: make(chan struct{})}
	value := startShutdownNode(t, filepath.Join(t.TempDir(), "profile"), script)
	caller := protocol.NodeID{38}
	if err := value.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		return store.PairLinkPeer(t.Context(), db, caller, link.DirectionController)
	}); err != nil {
		t.Fatal(err)
	}
	config := sessionConfigFixtureFor(t, value, caller)
	created := dispatchFixture(t, value, caller, protocol.NewRequest(value.ID(), protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Config: &config}}))
	if created.Fault != nil {
		t.Fatal(created.Fault)
	}
	session := created.Output.Data.(protocol.Session)
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
	defer cancel()
	connect := func() (*client.Client, <-chan struct{}) {
		t.Helper()
		controller := client.New(link.NewLocal(value.ID(), caller, value))
		t.Cleanup(func() { _ = controller.Close() })
		if err := controller.Start(ctx); err != nil {
			t.Fatal(err)
		}
		changed := make(chan struct{}, 1)
		_, err := controller.WatchConversation(ctx, session.ID, func() {
			select {
			case changed <- struct{}{}:
			default:
			}
		})
		if err != nil {
			t.Fatal(err)
		}
		return controller, changed
	}
	controller, _ := connect()
	output, err := controller.Do(ctx, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{
		Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Continue after the controller closes"},
	}})
	if err != nil {
		t.Fatal(err)
	}
	turn := output.Data.(protocol.QueuedTurn)
	var running context.Context
	select {
	case running = <-script.started:
	case <-ctx.Done():
		t.Fatal("model did not start")
	}
	if err := controller.Close(); err != nil {
		t.Fatal(err)
	}
	if running.Err() != nil || value.ctx.Err() != nil {
		t.Fatal("closing the controller cancelled its execution Node or admitted turn")
	}
	fresh, changed := connect()
	wait := func(status protocol.ConversationStatus) {
		t.Helper()
		for {
			if state, ok := fresh.Conversation(session.ID); ok {
				if state.Err != nil {
					t.Fatal(state.Err)
				}
				for _, run := range state.Snapshot.Page.Runs {
					if run.Turn == turn.ID && run.Status == status {
						return
					}
				}
			}
			select {
			case <-changed:
			case <-ctx.Done():
				t.Fatalf("reconnected controller did not observe %s", status)
			}
		}
	}
	wait(protocol.ConversationStatusRunning)
	close(script.finish)
	wait(protocol.ConversationStatusCompleted)
	if calls := script.calls.Load(); calls != 1 {
		t.Fatalf("controller reconnection replayed the turn: %d calls", calls)
	}
}

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
		// A file the profile's next owner wrote: its row is not this
		// process's to judge, so no collection of the abandoned sequence
		// may delete it. The database stayed open, so one could.
		attachments := filepath.Join(value.profile.Ownership.Path, "attachments")
		if err := os.MkdirAll(attachments, 0o700); err != nil {
			t.Fatal(err)
		}
		unbound := filepath.Join(attachments, protocol.NewAttachmentID().String())
		if err := os.WriteFile(unbound, []byte("written after the release"), 0o600); err != nil {
			t.Fatal(err)
		}
		close(release)
		if err := <-read; err != nil {
			t.Fatalf("a read in progress lost its connection: %v", err)
		}
		value.work.Done()
		// The sequence ends by closing the database it kept open.
		for started := time.Now(); ; time.Sleep(10 * time.Millisecond) {
			err := value.profile.Store.Node.Read(ctx, func(*sql.Conn) error { return nil })
			if err != nil {
				break
			}
			if time.Since(started) > 10*time.Second {
				t.Fatal("the abandoned sequence did not end")
			}
		}
		if _, err := os.Stat(unbound); err != nil {
			t.Fatalf("a collection ran after the profile was released: %v", err)
		}
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
		filepath.Dir(write("tmp/backup-1/node.turso", 0o600)),
		filepath.Dir(write("tmp/restore-1/storage/node.turso", 0o600)),
		write("tmp/command-output-1", 0o600),
		filepath.Dir(write("storage/.upgrade-1/storage/node.turso", 0o600)),
		filepath.Dir(write("storage/.schema-reference-1/reference.turso", 0o600)),
		filepath.Dir(filepath.Dir(write("storage/backups/.upgrade-1/storage/node.turso", 0o600))),
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
	outside := filepath.Join(t.TempDir(), "outside")
	if err := os.WriteFile(outside, []byte("kept"), 0o600); err != nil {
		t.Fatal(err)
	}
	link := filepath.Join(path, "tmp", "link")
	linkErr := os.Symlink(outside, link)
	kept := []string{
		// The process service kills the group this names, then removes it.
		write("tmp/command-group-1", 0o600),
		write("storage/backups/node-before-upgrade-1/node.turso", 0o600),
		write("storage/.upgraded", 0o600),
		write("plugins/packages/published/plugin.json", 0o600),
		write("python/environments/published/pyvenv.cfg", 0o600),
		filepath.Join(path, "storage", "node.turso"),
		filepath.Join(path, "link.key"),
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
	t.Run("symlink", func(t *testing.T) {
		if linkErr != nil {
			const errorPrivilegeNotHeld = syscall.Errno(1314)
			if runtime.GOOS == "windows" && errors.Is(linkErr, errorPrivilegeNotHeld) {
				t.Skipf("Windows symlink privilege is unavailable: %v", linkErr)
			}
			t.Fatal(linkErr)
		}
		if _, err := os.Lstat(link); !errors.Is(err, os.ErrNotExist) {
			t.Errorf("%s survived the start: %v", link, err)
		}
		if _, err := os.Lstat(outside); err != nil {
			t.Errorf("the start removed %s: %v", outside, err)
		}
	})
	if second.Identity.ID() != first.Identity.ID() {
		t.Error("the profile changed its identity")
	}
}
