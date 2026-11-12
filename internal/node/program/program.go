// Package program starts and owns the executables a package declares. It
// knows no protocol: a caller receives the child's standard streams and
// speaks whatever the program speaks.
package program

import (
	"bufio"
	"errors"
	"io"
	"log/slog"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"sync"
)

// inherited are the variables a program receives from the Node's own
// environment: the search path, the account's directories and session bus,
// locale, temporary directory and proxy settings. Nothing else is inherited.
var inherited = []string{
	"PATH", "HOME", "USER", "LOGNAME", "LANG", "LC_ALL", "TMPDIR", "TEMP", "TMP",
	"USERPROFILE", "APPDATA", "LOCALAPPDATA", "SystemRoot", "SystemDrive", "PATHEXT",
	"XDG_RUNTIME_DIR", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME", "DBUS_SESSION_BUS_ADDRESS",
	"HTTP_PROXY", "HTTPS_PROXY", "NO_PROXY", "http_proxy", "https_proxy", "no_proxy",
}

// Inherited reports whether the Node owns name, so a declaration cannot set it.
func Inherited(name string) bool {
	return slices.ContainsFunc(inherited, func(owned string) bool { return strings.EqualFold(owned, name) })
}

// File is written into the program's directory before it starts.
type File struct{ Path, Content string }

// Spec is one start of a declared program. Dir is the program's own
// directory: its working directory and the only place Files are written.
type Spec struct {
	Command string
	Args    []string
	Env     map[string]string
	Files   []File
	Dir     string
	// Host is the Node's environment as NAME=value entries.
	Host []string
	// Log names the program in the Node's diagnostics.
	Log string
}

// ErrNotInstalled reports that the command is not on the search path.
var ErrNotInstalled = errors.New("program is not installed")

// Resolve finds an executable by name on the PATH of environment. A name
// with a separator is refused, so a declaration never names a path.
func Resolve(name string, environment map[string]string) (string, error) {
	if name == "" || strings.ContainsAny(name, `/\`) {
		return "", errors.New("program must be an executable name")
	}
	names := []string{name}
	if runtime.GOOS == "windows" && filepath.Ext(name) == "" {
		for _, extension := range strings.Split(environment["PATHEXT"], ";") {
			names = append(names, name+strings.ToLower(extension))
		}
	}
	for _, directory := range filepath.SplitList(environment["PATH"]) {
		if !filepath.IsAbs(directory) {
			continue
		}
		for _, candidate := range names {
			path := filepath.Join(directory, candidate)
			info, err := os.Stat(path)
			if err == nil && info.Mode().IsRegular() && (runtime.GOOS == "windows" || info.Mode().Perm()&0111 != 0) {
				return path, nil
			}
		}
	}
	return "", ErrNotInstalled
}

// Process is a running program with piped standard streams. A program that
// needs a terminal instead of pipes is not built: it would be a second start
// function here returning the same Process over the PTY that
// internal/node/terminal owns, with Pool unchanged.
type Process struct {
	Stdin  io.WriteCloser
	Stdout io.Reader
	kill   func()
	done   chan struct{}
	once   sync.Once
	mu     sync.Mutex
	tail   string
}

// Start writes the declared files, then runs the program in its directory
// with a scrubbed environment, as the leader of its own process group. Its
// standard error goes to the Node's log line by line.
func Start(spec Spec) (*Process, error) {
	environment := map[string]string{}
	for _, entry := range spec.Host {
		if name, value, ok := strings.Cut(entry, "="); ok && slices.Contains(inherited, name) {
			environment[name] = value
		}
	}
	executable, err := Resolve(spec.Command, environment)
	if err != nil {
		return nil, err
	}
	for name, value := range spec.Env {
		if !Inherited(name) {
			environment[name] = value
		}
	}
	if err := os.MkdirAll(spec.Dir, 0700); err != nil {
		return nil, err
	}
	root, err := os.OpenRoot(spec.Dir)
	if err != nil {
		return nil, err
	}
	for _, file := range spec.Files {
		if directory := filepath.Dir(file.Path); directory != "." {
			err = root.MkdirAll(directory, 0700)
		}
		if err == nil {
			err = root.WriteFile(file.Path, []byte(file.Content), 0600)
		}
		if err != nil {
			root.Close()
			return nil, err
		}
	}
	root.Close()
	command := exec.Command(executable, spec.Args...)
	command.Dir = spec.Dir
	for name, value := range environment {
		command.Env = append(command.Env, name+"="+value)
	}
	stdin, err := command.StdinPipe()
	if err != nil {
		return nil, err
	}
	stdout, err := command.StdoutPipe()
	if err != nil {
		return nil, err
	}
	stderr, err := command.StderrPipe()
	if err != nil {
		return nil, err
	}
	kill, err := StartGroup(command)
	if err != nil {
		return nil, err
	}
	process := &Process{Stdin: stdin, Stdout: stdout, kill: kill, done: make(chan struct{})}
	drained := make(chan struct{})
	go func() {
		defer close(drained)
		lines := bufio.NewScanner(stderr)
		lines.Buffer(make([]byte, 0, 64*1024), 1024*1024)
		for lines.Scan() {
			line := lines.Text()
			slog.Debug("Program diagnostic", "program", spec.Log, "line", line)
			process.mu.Lock()
			process.tail = line
			process.mu.Unlock()
		}
	}()
	go func() {
		<-drained
		_ = command.Wait()
		kill()
		close(process.done)
	}()
	return process, nil
}

// Done closes when the program has exited and its group is gone.
func (process *Process) Done() <-chan struct{} { return process.done }

// Diagnostic is the last line the program wrote to standard error.
func (process *Process) Diagnostic() string {
	process.mu.Lock()
	defer process.mu.Unlock()
	return process.tail
}

// Kill ends the program's whole process group and waits for it.
func (process *Process) Kill() {
	process.once.Do(func() {
		_ = process.Stdin.Close()
		process.kill()
	})
	<-process.done
}
