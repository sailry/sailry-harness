package program

import (
	"bufio"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"syscall"
	"testing"
	"time"
)

// The test binary is its own program: with PROGRAM_FIXTURE set it reports
// its start on standard output, then echoes lines until its input closes.
func TestMain(suite *testing.M) {
	if mode, fixture := os.LookupEnv("PROGRAM_FIXTURE"); fixture {
		directory, _ := os.Getwd()
		fmt.Fprintf(os.Stdout, "%d|%s|%s|%s\n", os.Getpid(), directory, strings.Join(os.Args[1:], " "), os.Getenv("PROGRAM_LEAK"))
		fmt.Fprintln(os.Stderr, "fixture diagnostic")
		if mode == "exit" {
			os.Exit(3)
		}
		lines := bufio.NewScanner(os.Stdin)
		for lines.Scan() {
			fmt.Fprintln(os.Stdout, "echo "+lines.Text())
		}
		return
	}
	os.Exit(suite.Run())
}

func fixtureSpec(t *testing.T, mode string) Spec {
	t.Helper()
	if runtime.GOOS == "windows" {
		t.Skip("the fixture program is linked by a symbolic link")
	}
	self, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	bin := t.TempDir()
	if err := os.Symlink(self, filepath.Join(bin, "fixture-program")); err != nil {
		t.Fatal(err)
	}
	return Spec{Command: "fixture-program", Args: []string{"serve"}, Env: map[string]string{"PROGRAM_FIXTURE": mode, "PATH": "/nowhere"},
		Dir: filepath.Join(t.TempDir(), "programs", "fixture"), Host: []string{"PATH=" + bin, "PROGRAM_LEAK=host"}, Log: "fixture/program",
		Files: []File{{Path: filepath.Join("config", "agent.md"), Content: "declared"}}}
}

func alive(pid int) bool {
	process, err := os.FindProcess(pid)
	return err == nil && process.Signal(syscall.Signal(0)) == nil
}

func gone(t *testing.T, pid int) {
	t.Helper()
	for deadline := time.Now().Add(10 * time.Second); alive(pid); time.Sleep(10 * time.Millisecond) {
		if time.Now().After(deadline) {
			t.Fatalf("process %d is still running", pid)
		}
	}
}

func started(t *testing.T, process *Process) (pid int, reader *bufio.Reader, fields []string) {
	t.Helper()
	reader = bufio.NewReader(process.Stdout)
	line, err := reader.ReadString('\n')
	if err != nil {
		t.Fatal(err)
	}
	fields = strings.Split(strings.TrimSpace(line), "|")
	if _, err := fmt.Sscan(fields[0], &pid); err != nil {
		t.Fatal(err)
	}
	return pid, reader, fields
}

func TestStart(t *testing.T) {
	t.Run("runs in its directory with a scrubbed environment", func(t *testing.T) {
		spec := fixtureSpec(t, "serve")
		process, err := Start(spec)
		if err != nil {
			t.Fatal(err)
		}
		pid, reader, fields := started(t, process)
		directory, _ := filepath.EvalSymlinks(spec.Dir)
		reported, _ := filepath.EvalSymlinks(fields[1])
		if reported != directory || fields[2] != "serve" || fields[3] != "" {
			t.Fatalf("start = %q", fields)
		}
		if written, err := os.ReadFile(filepath.Join(spec.Dir, "config", "agent.md")); err != nil || string(written) != "declared" {
			t.Fatalf("declared file = %q, %v", written, err)
		}
		if _, err := process.Stdin.Write([]byte("ping\n")); err != nil {
			t.Fatal(err)
		}
		if line, err := reader.ReadString('\n'); err != nil || line != "echo ping\n" {
			t.Fatalf("echo = %q, %v", line, err)
		}
		process.Kill()
		gone(t, pid)
		if process.Diagnostic() != "fixture diagnostic" {
			t.Fatalf("diagnostic = %q", process.Diagnostic())
		}
	})
	t.Run("reports a program that exits", func(t *testing.T) {
		process, err := Start(fixtureSpec(t, "exit"))
		if err != nil {
			t.Fatal(err)
		}
		select {
		case <-process.Done():
		case <-time.After(10 * time.Second):
			t.Fatal("the exit was not reported")
		}
		process.Kill()
	})
	t.Run("tells a missing program", func(t *testing.T) {
		spec := fixtureSpec(t, "serve")
		spec.Host = []string{"PATH=" + t.TempDir()}
		if _, err := Start(spec); !errors.Is(err, ErrNotInstalled) {
			t.Fatalf("missing program: %v", err)
		}
		spec.Command = "../fixture-program"
		if _, err := Start(spec); err == nil || errors.Is(err, ErrNotInstalled) {
			t.Fatalf("path as a command: %v", err)
		}
	})
	t.Run("writes no file outside its directory", func(t *testing.T) {
		spec := fixtureSpec(t, "serve")
		spec.Files = []File{{Path: filepath.Join("..", "escaped.md"), Content: "x"}}
		if process, err := Start(spec); err == nil {
			process.Kill()
			t.Fatal("a file escaped the program directory")
		}
		if _, err := os.Stat(filepath.Join(filepath.Dir(spec.Dir), "escaped.md")); err == nil {
			t.Fatal("a file escaped the program directory")
		}
	})
}

func TestPool(t *testing.T) {
	launch := func(t *testing.T, pids *[]int) func() (*Process, any, error) {
		return func() (*Process, any, error) {
			process, err := Start(fixtureSpec(t, "serve"))
			if err != nil {
				return nil, nil, err
			}
			pid, _, _ := started(t, process)
			*pids = append(*pids, pid)
			return process, pid, nil
		}
	}
	idle := func(*Entry) int { return 1 }

	t.Run("reuses, bounds and replaces an owner's programs", func(t *testing.T) {
		var pids []int
		pool := NewPool(2, time.Minute)
		defer pool.Close()
		first, err := pool.Lease("owner", idle, launch(t, &pids))
		if err != nil {
			t.Fatal(err)
		}
		second, err := pool.Lease("owner", idle, launch(t, &pids))
		if err != nil || len(pids) != 2 {
			t.Fatalf("second program: %v, %v", pids, err)
		}
		if _, err := pool.Lease("owner", idle, launch(t, &pids)); !errors.Is(err, ErrBusy) {
			t.Fatalf("lease beyond the limit: %v", err)
		}
		// Another owner has its own bound.
		other, err := pool.Lease("other", idle, launch(t, &pids))
		if err != nil || len(pids) != 3 {
			t.Fatalf("other owner: %v, %v", pids, err)
		}
		pool.Release(other, true)
		pool.Release(first, true)
		again, err := pool.Lease("owner", idle, launch(t, &pids))
		if err != nil || again != first || len(pids) != 3 {
			t.Fatalf("reuse: %v, %v", pids, err)
		}
		pool.Release(again, true)
		// No idle program is wanted: the idle one is replaced.
		replaced, err := pool.Lease("owner", func(*Entry) int { return 0 }, launch(t, &pids))
		if err != nil || len(pids) != 4 {
			t.Fatalf("replacement: %v, %v", pids, err)
		}
		gone(t, pids[0])
		pool.Release(replaced, false)
		gone(t, pids[3])
		pool.Release(second, true)
		pool.Invalidate("owner")
		gone(t, pids[1])
		if !alive(pids[2]) {
			t.Fatal("another owner's program was stopped")
		}
		pool.Close()
		gone(t, pids[2])
		if _, err := pool.Lease("owner", idle, launch(t, &pids)); err == nil {
			t.Fatal("a closed pool started a program")
		}
	})

	t.Run("stops idle programs and leaves leased ones", func(t *testing.T) {
		var pids []int
		pool := NewPool(2, 50*time.Millisecond)
		defer pool.Close()
		unused, err := pool.Lease("owner", idle, launch(t, &pids))
		if err != nil {
			t.Fatal(err)
		}
		held, err := pool.Lease("owner", idle, launch(t, &pids))
		if err != nil {
			t.Fatal(err)
		}
		pool.Release(unused, true)
		gone(t, pids[0])
		if !alive(pids[1]) {
			t.Fatal("a leased program was stopped")
		}
		pool.Release(held, true)
		gone(t, pids[1])
	})

	t.Run("frees the slot of a program that did not start", func(t *testing.T) {
		pool := NewPool(1, time.Minute)
		defer pool.Close()
		failed := errors.New("no start")
		if _, err := pool.Lease("owner", idle, func() (*Process, any, error) { return nil, nil, failed }); !errors.Is(err, failed) {
			t.Fatalf("failed start: %v", err)
		}
		var pids []int
		if entry, err := pool.Lease("owner", idle, launch(t, &pids)); err != nil {
			t.Fatal(err)
		} else {
			pool.Release(entry, true)
		}
	})
}
