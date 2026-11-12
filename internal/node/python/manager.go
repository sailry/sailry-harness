package python

import (
	"context"
	"errors"
	"fmt"
	"maps"
	"net/url"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type Options struct {
	RuntimeDirectory string
	Environment      []string
	Run              func(context.Context, Command) error
	Now              func() time.Time
	Changed          func([]protocol.PythonPreparation)
}

type prepared struct {
	permit chan struct{}
	mu     sync.Mutex
	cache  hashCache
}

type active struct {
	stage    protocol.PythonStage
	packages map[string]int
}

type Manager struct {
	profile    string
	root       string
	options    Options
	runtimeMu  sync.Mutex
	runtime    *Runtime
	checked    bool
	notify     sync.Mutex
	mu         sync.Mutex
	index      *string
	prepared   map[string]*prepared
	leases     map[string]int
	active     map[string]*active
	references []Requirements
	retained   bool
	collected  time.Time
}

type Ready struct {
	Python      string
	Environment string
	Runtime     string
	requirement *Requirements
	manager     *Manager
	key         string
	once        sync.Once
}

type Access struct {
	Ready     *Ready
	Directory string
}

func NewManager(profile string, options Options) (*Manager, error) {
	root, err := filepath.EvalSymlinks(profile)
	if err != nil || !filepath.IsAbs(root) {
		return nil, invalid("Python requires an absolute Node profile")
	}
	if options.Run == nil {
		options.Run = Run
	}
	if options.Now == nil {
		options.Now = time.Now
	}
	options.Environment = slices.Clone(options.Environment)
	pythonRoot := filepath.Join(root, "python")
	for _, directory := range []string{pythonRoot, filepath.Join(pythonRoot, "environments"), filepath.Join(pythonRoot, "staging"), filepath.Join(pythonRoot, "cache")} {
		if err := core.PrivateDirectory(directory); err != nil {
			return nil, storage(err)
		}
	}
	return &Manager{
		profile: root, root: pythonRoot, options: options,
		prepared: map[string]*prepared{}, leases: map[string]int{}, active: map[string]*active{},
	}, nil
}

func ValidateIndex(index *string) error {
	if index == nil {
		return nil
	}
	target, err := url.Parse(*index)
	if err != nil || target.Scheme != "http" && target.Scheme != "https" || target.Hostname() == "" || target.User != nil || strings.Contains(*index, "#") || target.RawQuery != "" || target.ForceQuery || len(*index) > 4096 {
		return invalid("Python package index must be an HTTP URL without credentials")
	}
	return nil
}

func (manager *Manager) Configure(index *string) error {
	if err := ValidateIndex(index); err != nil {
		return err
	}
	manager.mu.Lock()
	defer manager.mu.Unlock()
	manager.index = copyString(index)
	return nil
}

func copyString(value *string) *string {
	if value == nil {
		return nil
	}
	copied := *value
	return &copied
}

func (manager *Manager) load(ctx context.Context) (*Runtime, error) {
	manager.runtimeMu.Lock()
	defer manager.runtimeMu.Unlock()
	if err := check(ctx); err != nil {
		return nil, err
	}
	if manager.runtime == nil {
		var value *Runtime
		var err error
		if manager.options.RuntimeDirectory == "" {
			value, err = BundledRuntime()
		} else {
			value, err = OpenRuntime(manager.options.RuntimeDirectory)
		}
		if err != nil {
			return nil, err
		}
		manager.runtime = value
	}
	if err := manager.runtime.Verify(ctx); err != nil {
		return nil, err
	}
	if !manager.checked {
		if err := manager.options.Run(ctx, manager.command(manager.runtime.Python, []string{
			"-I", "-c", "import sys,venv,pip; assert sys.version.split()[0] == '" + Version + "', 'bundled Python version mismatch'",
		}, manager.runtime.Root)); err != nil {
			return nil, err
		}
		manager.checked = true
	}
	return manager.runtime, nil
}

func (manager *Manager) Ensure(ctx context.Context, requirement *Requirements) (*Ready, error) {
	runtime, err := manager.load(ctx)
	if err != nil {
		return nil, err
	}
	if requirement == nil {
		return &Ready{Python: runtime.Python, Environment: runtime.Root, Runtime: runtime.Root}, nil
	}
	captured, err := NewRequirements(requirement.Package, requirement.Content)
	if err != nil {
		return nil, err
	}
	key := captured.Key(runtime.Identity)
	path := filepath.Join(manager.root, "environments", key)
	manager.mu.Lock()
	entry := manager.prepared[key]
	if entry == nil {
		entry = &prepared{permit: make(chan struct{}, 1), cache: hashCache{}}
		manager.prepared[key] = entry
	}
	manager.leases[key]++
	activity := manager.active[key]
	if activity == nil {
		activity = &active{packages: map[string]int{}}
		manager.active[key] = activity
	}
	activity.packages[captured.Package.Name]++
	manager.mu.Unlock()
	manager.changed()
	success := false
	defer func() {
		manager.releaseRequest(key, captured.Package.Name)
		if !success {
			manager.releaseLease(key)
		}
	}()
	valid, err := manager.valid(ctx, entry, path, runtime.Root, key)
	if err != nil {
		return nil, err
	}
	if !valid {
		select {
		case entry.permit <- struct{}{}:
		case <-ctx.Done():
			return nil, cancelled()
		}
		defer func() { <-entry.permit }()
		valid, err = manager.valid(ctx, entry, path, runtime.Root, key)
		if err != nil {
			return nil, err
		}
		if !valid {
			defer manager.report(key, "")
			if err := manager.prepare(ctx, captured, runtime, path, key); err != nil {
				return nil, err
			}
			valid, err = manager.valid(ctx, entry, path, runtime.Root, key)
			if err != nil {
				return nil, err
			}
			if !valid {
				return nil, unavailable("prepared Python environment failed validation")
			}
		}
	}
	if err := check(ctx); err != nil {
		return nil, err
	}
	used := filepath.Join(path, ".sailry-used")
	if err := writePrivate(used, nil); err != nil {
		return nil, err
	}
	now := manager.options.Now()
	if err := os.Chtimes(used, now, now); err != nil {
		return nil, storage(err)
	}
	if err := manager.Collect(ctx, key); err != nil && ctx.Err() != nil {
		return nil, err
	}
	success = true
	return &Ready{Python: environmentInterpreter(path), Environment: path, Runtime: runtime.Root, requirement: captured, manager: manager, key: key}, nil
}

func (manager *Manager) valid(ctx context.Context, entry *prepared, path, runtime, key string) (bool, error) {
	entry.mu.Lock()
	defer entry.mu.Unlock()
	return validEnvironment(ctx, path, runtime, key, entry.cache)
}

func (ready *Ready) Close() error {
	if ready == nil {
		return nil
	}
	ready.once.Do(func() {
		if ready.manager != nil {
			ready.manager.releaseLease(ready.key)
		}
	})
	return nil
}

func (manager *Manager) releaseLease(key string) {
	manager.mu.Lock()
	defer manager.mu.Unlock()
	manager.leases[key]--
	if manager.leases[key] == 0 {
		delete(manager.leases, key)
	}
}

func (manager *Manager) Refresh(ctx context.Context, ready *Ready) (*Ready, error) {
	if ready == nil {
		return nil, invalid("Python capture is unavailable")
	}
	return manager.Ensure(ctx, ready.requirement)
}

func (manager *Manager) releaseRequest(key, packageName string) {
	manager.mu.Lock()
	entry := manager.active[key]
	if entry != nil {
		entry.packages[packageName]--
		if entry.packages[packageName] == 0 {
			delete(entry.packages, packageName)
		}
		if entry.stage == "" && len(entry.packages) == 0 {
			delete(manager.active, key)
		}
	}
	manager.mu.Unlock()
	manager.changed()
}

func (manager *Manager) report(key string, stage protocol.PythonStage) {
	manager.mu.Lock()
	entry := manager.active[key]
	if entry != nil {
		entry.stage = stage
		if stage == "" && len(entry.packages) == 0 {
			delete(manager.active, key)
		}
	}
	manager.mu.Unlock()
	manager.changed()
}

func (manager *Manager) Preparations() []protocol.PythonPreparation {
	manager.mu.Lock()
	defer manager.mu.Unlock()
	result := make([]protocol.PythonPreparation, 0)
	for _, key := range slices.Sorted(maps.Keys(manager.active)) {
		entry := manager.active[key]
		if entry.stage == "" {
			continue
		}
		for _, packageName := range slices.Sorted(maps.Keys(entry.packages)) {
			result = append(result, protocol.PythonPreparation{Package: packageName, Stage: entry.stage})
		}
	}
	return result
}

func (manager *Manager) changed() {
	if manager.options.Changed != nil {
		manager.notify.Lock()
		defer manager.notify.Unlock()
		manager.options.Changed(manager.Preparations())
	}
}

func (manager *Manager) Retain(requirements []Requirements) {
	manager.mu.Lock()
	defer manager.mu.Unlock()
	manager.references = slices.Clone(requirements)
	manager.retained = true
}

func (manager *Manager) Collect(ctx context.Context, current string) error {
	manager.runtimeMu.Lock()
	runtime := manager.runtime
	manager.runtimeMu.Unlock()
	if runtime == nil {
		return nil
	}
	manager.mu.Lock()
	defer manager.mu.Unlock()
	now := manager.options.Now()
	if !manager.retained || !manager.collected.IsZero() && now.Sub(manager.collected) < 24*time.Hour {
		return nil
	}
	manager.collected = now
	retained := make(map[string]bool, len(manager.references))
	for _, requirement := range manager.references {
		retained[requirement.Key(runtime.Identity)] = true
	}
	entries, err := os.ReadDir(filepath.Join(manager.root, "environments"))
	if err != nil {
		return storage(err)
	}
	for _, entry := range entries {
		if err := check(ctx); err != nil {
			return err
		}
		key := entry.Name()
		if key == current || retained[key] || !keyValid(key) || manager.leases[key] != 0 {
			continue
		}
		path := filepath.Join(manager.root, "environments", key)
		info, err := os.Lstat(path)
		if err != nil || !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
			continue
		}
		used, err := os.Lstat(filepath.Join(path, ".sailry-used"))
		if err != nil || !used.Mode().IsRegular() || used.Mode()&os.ModeSymlink != 0 || now.Sub(used.ModTime()) < 7*24*time.Hour {
			continue
		}
		if err := os.RemoveAll(path); err != nil {
			return storage(err)
		}
		delete(manager.prepared, key)
	}
	return nil
}

func check(ctx context.Context) error {
	if ctx.Err() != nil {
		return cancelled()
	}
	return nil
}

func invalid(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: message}
}
func cancelled() error {
	return &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "Python environment preparation was cancelled"}
}
func unavailable(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: message}
}
func notConfigured() error {
	return &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "bundled Python runtime is unavailable"}
}
func storage(err error) error {
	var fault *protocol.Fault
	if errors.As(err, &fault) {
		return fault
	}
	return unavailable(fmt.Sprintf("Python environment storage failed: %v", err))
}

func (ready *Ready) MCP(command string, arguments []string, environment map[string]string, inheritedPath string) (string, []string, map[string]string, error) {
	if command != "python" && command != "python3" {
		return command, arguments, environment, nil
	}
	if ready == nil {
		return "", nil, nil, notConfigured()
	}
	values := maps.Clone(environment)
	if values == nil {
		values = map[string]string{}
	}
	values["PYTHONDONTWRITEBYTECODE"] = "1"
	path := values["PATH"]
	if path == "" {
		path = inheritedPath
	}
	values["PATH"] = strings.Join(append([]string{filepath.Dir(ready.Python)}, filepath.SplitList(path)...), string(os.PathListSeparator))
	return ready.Python, append([]string{"-B"}, arguments...), values, nil
}
