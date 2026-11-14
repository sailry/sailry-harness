// Package logging owns the process's rotating diagnostics. An entry point
// installs it once; a Node never installs a handler. Records contain
// identifiers and lifecycle metadata, never operation payloads.
package logging

import (
	"context"
	"errors"
	"fmt"
	"log"
	"log/slog"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/sailry/sailry-harness/internal/core"
	"gopkg.in/natefinch/lumberjack.v2"
)

// Bounded at 100 MiB per application while retaining several days of
// ordinary use: ten files of ten mebibytes, rotated by size only.
const (
	maxFileMegabytes = 10
	retainedFiles    = 10
	current          = "current.log"
	lockName         = "writer.lock"
)

// Target is the message of every record this package owns. The handler
// writes those to the file and passes all others to the handler it replaced.
const Target = "sailry"

// ErrUnavailable reports that logging could not be installed.
var ErrUnavailable = errors.New("logging is unavailable")

type installation struct {
	directory   string
	application string
	file        *lumberjack.Logger
	lease       *os.File
	previous    *slog.Logger
	mu          sync.Mutex
	failed      bool
	errors      int
}

var (
	installMu sync.Mutex
	installed atomic.Pointer[installation]
	report    atomic.Pointer[func(slog.Level, Fields)]
)

// Install starts the application's log under the profile at
// logs/<application>/current.log and makes it the slog default. application
// is "desktop" or "host". The returned function ends it after the Node has
// stopped. A second installation in one process fails.
func Install(profile, application string) (func(), error) {
	installMu.Lock()
	defer installMu.Unlock()
	if installed.Load() != nil || application != "desktop" && application != "host" {
		return nil, ErrUnavailable
	}
	directory := filepath.Join(profile, "logs", application)
	// Logging precedes Node startup, so it must create the profile with the
	// same privacy as the Node instead of leaving an inherited Windows DACL.
	for _, path := range []string{profile, filepath.Join(profile, "logs"), directory} {
		if err := core.PrivateDirectory(path); err != nil {
			return nil, ErrUnavailable
		}
	}
	// The Node names its profile by the resolved path.
	directory, err := filepath.EvalSymlinks(directory)
	if err != nil {
		return nil, ErrUnavailable
	}
	if info, err := os.Lstat(filepath.Join(directory, lockName)); err == nil && !info.Mode().IsRegular() {
		return nil, ErrUnavailable
	}
	// One process writes a stream: a second writer would rotate under the first.
	lease, err := lockFile(filepath.Join(directory, lockName))
	if err != nil {
		return nil, ErrUnavailable
	}
	path := filepath.Join(directory, current)
	if info, err := os.Lstat(path); err == nil && (!info.Mode().IsRegular() || info.Size() > maxFileMegabytes*1024*1024) || err != nil && !errors.Is(err, os.ErrNotExist) {
		_ = lease.Close()
		return nil, ErrUnavailable
	}
	value := &installation{directory: directory, application: application, lease: lease, previous: slog.Default(),
		file: &lumberjack.Logger{Filename: path, MaxSize: maxFileMegabytes, MaxBackups: retainedFiles - 1}}
	installed.Store(value)
	// Replacing the default handler routes the log package into slog, and
	// the handler it replaced writes through the log package: keep the log
	// package's own output so other records still reach it without a loop.
	output, flags := log.Writer(), log.Flags()
	slog.SetDefault(slog.New(&handler{owner: value, next: value.previous.Handler()}))
	log.SetOutput(output)
	log.SetFlags(flags)
	return func() {
		installMu.Lock()
		defer installMu.Unlock()
		if installed.Load() != value {
			return
		}
		slog.SetDefault(value.previous)
		installed.Store(nil)
		value.mu.Lock()
		_ = value.file.Close()
		failures := value.errors
		value.mu.Unlock()
		_ = value.lease.Close()
		if failures > 0 {
			fmt.Fprintf(os.Stderr, "sailry logging encountered %d diagnostic I/O failures\n", failures)
		}
	}, nil
}

// Directory is the profile's logs directory, "" before Install.
func Directory() string {
	if value := installed.Load(); value != nil {
		return filepath.Dir(value.directory)
	}
	return ""
}

// CurrentFile is the active file, or the latest earlier daily file before
// the first record. It is "" when there is none.
func CurrentFile() (string, error) {
	value := installed.Load()
	if value == nil {
		return "", nil
	}
	path := filepath.Join(value.directory, current)
	if info, err := os.Lstat(path); err == nil && info.Mode().IsRegular() {
		return path, nil
	}
	entries, err := os.ReadDir(value.directory)
	if err != nil {
		return "", err
	}
	latest := ""
	for _, entry := range entries {
		if entry.Type().IsRegular() && daily(entry.Name()) && entry.Name() > latest {
			latest = entry.Name()
		}
	}
	if latest == "" {
		return "", nil
	}
	return filepath.Join(value.directory, latest), nil
}

func daily(name string) bool {
	date, found := strings.CutSuffix(name, ".log")
	if !found || len(date) != 10 {
		return false
	}
	_, err := time.Parse("2006-01-02", date)
	return err == nil
}

// Retain retires earlier daily files. A Node calls it only after it owns the
// profile, so a rejected second start cannot retire a running process's file.
// Other files, directories and links in the directory remain untouched.
func Retain(profile string) error {
	value := installed.Load()
	if value == nil {
		return nil
	}
	owner, err := filepath.EvalSymlinks(filepath.Dir(filepath.Dir(value.directory)))
	if err != nil || owner != profile {
		return nil
	}
	entries, err := os.ReadDir(value.directory)
	if err != nil {
		return err
	}
	for _, entry := range entries {
		if entry.Type().IsRegular() && daily(entry.Name()) {
			if err := os.Remove(filepath.Join(value.directory, entry.Name())); err != nil {
				return err
			}
		}
	}
	return nil
}

// SetReport names the receiver of warning and error records, nil for none.
// It is called on the logging goroutine and must not block.
func SetReport(receive func(slog.Level, Fields)) {
	if receive == nil {
		report.Store(nil)
		return
	}
	report.Store(&receive)
}

// Emit writes one record. It does nothing before Install.
func Emit(level slog.Level, attrs ...slog.Attr) {
	if installed.Load() != nil {
		slog.LogAttrs(context.Background(), level, Target, attrs...)
	}
}

type handler struct {
	owner *installation
	next  slog.Handler
}

func (h *handler) Enabled(ctx context.Context, level slog.Level) bool {
	return level >= slog.LevelDebug || h.next.Enabled(ctx, level)
}

func (h *handler) WithAttrs(attrs []slog.Attr) slog.Handler {
	return &handler{owner: h.owner, next: h.next.WithAttrs(attrs)}
}

func (h *handler) WithGroup(name string) slog.Handler {
	return &handler{owner: h.owner, next: h.next.WithGroup(name)}
}

func (h *handler) Handle(ctx context.Context, record slog.Record) error {
	if record.Message != Target {
		if h.next.Enabled(ctx, record.Level) {
			return h.next.Handle(ctx, record)
		}
		return nil
	}
	if record.Level < slog.LevelDebug {
		return nil
	}
	values := collect(record)
	if receive := report.Load(); receive != nil && record.Level >= slog.LevelWarn {
		(*receive)(record.Level, values)
	}
	at := record.Time
	if at.IsZero() {
		at = time.Now()
	}
	return h.owner.write(line(at, record.Level, h.owner.application, values))
}

// write appends a complete record. After an I/O failure the writer stops
// rather than risk a partial record or an exceeded limit.
func (value *installation) write(text string) error {
	value.mu.Lock()
	defer value.mu.Unlock()
	if value.failed {
		value.errors++
		return ErrUnavailable
	}
	if _, err := value.file.Write([]byte(text)); err != nil {
		value.failed = true
		if value.errors++; value.errors == 1 {
			fmt.Fprintln(os.Stderr, "sailry logging: diagnostic write failed")
		}
		return err
	}
	return nil
}
