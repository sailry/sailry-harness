package store

import (
	"context"
	"database/sql"
	"errors"
	"math"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/turso"
	"golang.org/x/sync/semaphore"
)

const allLeases = math.MaxInt64

// Handle owns one file's writer and its independent read-only connection pool.
type Handle struct {
	path    string
	leases  *semaphore.Weighted
	mu      sync.Mutex
	state   *connections
	stopped bool
}

type connections struct {
	writeDB *sql.DB
	readDB  *sql.DB
	writer  *writer
}

func OpenHandle(ctx context.Context, path string) (*Handle, error) {
	if !utf8.ValidString(path) {
		return nil, &core.InvalidProfileError{Path: path, Reason: "database path must be valid UTF-8"}
	}
	path, err := filepath.Abs(path)
	if err != nil {
		return nil, err
	}
	state, err := openConnections(ctx, path)
	if err != nil {
		return nil, err
	}
	return &Handle{path: path, leases: semaphore.NewWeighted(allLeases), state: state}, nil
}

func (handle *Handle) Path() string { return handle.path }

func (handle *Handle) Close() error {
	if err := handle.leases.Acquire(context.Background(), allLeases); err != nil {
		return err
	}
	return handle.closeOwned()
}

// closeIdle closes the handle unless a lease is held.
func (handle *Handle) closeIdle() error {
	if !handle.leases.TryAcquire(allLeases) {
		return nil
	}
	return handle.closeOwned()
}

// closeOwned closes the connections under every lease and returns the leases.
func (handle *Handle) closeOwned() error {
	defer handle.leases.Release(allLeases)
	handle.mu.Lock()
	defer handle.mu.Unlock()
	if handle.stopped {
		return nil
	}
	handle.stopped = true
	state := handle.state
	handle.state = nil
	if state == nil {
		return nil
	}
	return state.close()
}

func (handle *Handle) acquire(ctx context.Context) (*connections, error) {
	if err := handle.leases.Acquire(ctx, 1); err != nil {
		return nil, err
	}
	handle.mu.Lock()
	state, stopped := handle.state, handle.stopped
	handle.mu.Unlock()
	if stopped || state == nil {
		handle.leases.Release(1)
		return nil, core.ErrStopped
	}
	return state, nil
}

func openConnections(ctx context.Context, path string) (_ *connections, result error) {
	if err := checkDatabaseFiles(path); err != nil {
		return nil, err
	}
	initialized := false
	if info, err := os.Stat(path); err == nil {
		initialized = info.Size() != 0
	} else if !errors.Is(err, os.ErrNotExist) {
		return nil, err
	}
	if err := createDatabaseFile(path); err != nil {
		return nil, err
	}
	connector, err := turso.Connector(path, false)
	if err != nil {
		return nil, err
	}
	writeDB := sql.OpenDB(connector)
	writeDB.SetMaxOpenConns(1)
	writeDB.SetMaxIdleConns(1)
	defer func() {
		if result != nil {
			_ = writeDB.Close()
		}
	}()
	connection, err := writeDB.Conn(ctx)
	if err != nil {
		return nil, err
	}
	defer func() {
		if result != nil {
			_ = connection.Close()
		}
	}()
	var mode string
	journal := "PRAGMA journal_mode"
	if !initialized {
		journal = "PRAGMA journal_mode=mvcc"
	}
	if err := connection.QueryRowContext(ctx, journal).Scan(&mode); err != nil {
		return nil, err
	}
	if !strings.EqualFold(mode, "mvcc") && !strings.EqualFold(mode, "wal") {
		return nil, errors.New("Node database is not in MVCC or WAL mode")
	}
	if strings.EqualFold(mode, "mvcc") {
		// MVCC checkpoints run inside COMMIT. Keep batches near 512 KiB
		// without relaxing FULL durability.
		if _, err := connection.ExecContext(ctx, "PRAGMA mvcc_checkpoint_threshold=527360"); err != nil {
			return nil, err
		}
	}
	reader, err := turso.Connector(path, true)
	if err != nil {
		return nil, err
	}
	readDB := sql.OpenDB(reader)
	// A reader beyond the idle set is reopened with its pragmas on every read;
	// database/sql keeps only two by default.
	readDB.SetMaxIdleConns(8)
	var ready int
	if err := readDB.QueryRowContext(ctx, "SELECT 1").Scan(&ready); err != nil {
		_ = readDB.Close()
		return nil, err
	}
	if err := validateDatabasePrivacy(path); err != nil {
		_ = readDB.Close()
		return nil, err
	}
	state := &connections{
		writeDB: writeDB, readDB: readDB,
		writer: &writer{database: writeDB, connection: connection, jobs: make(chan *writeJob), stop: make(chan struct{}), done: make(chan struct{})},
	}
	go state.writer.run()
	return state, nil
}

func checkDatabaseFiles(path string) error {
	for _, file := range core.DatabaseFiles(path) {
		info, err := os.Lstat(file)
		if errors.Is(err, os.ErrNotExist) {
			continue
		}
		if err != nil {
			return err
		}
		if !info.Mode().IsRegular() {
			return &core.InvalidProfileError{Path: file, Reason: "database must be a regular file"}
		}
		if err := checkDatabaseLinks(file, info); err != nil {
			return err
		}
	}
	return nil
}

func (state *connections) close() error {
	close(state.writer.stop)
	<-state.writer.done
	return errors.Join(state.writer.connection.Close(), state.writeDB.Close(), state.readDB.Close())
}
