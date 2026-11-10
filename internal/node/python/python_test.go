package python

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPythonRequirementsValidation(test *testing.T) {
	test.Parallel()
	reference := protocol.PluginReference{Name: "fixture", Digest: strings.Repeat("a", 64), SettingsRevision: 1}
	for _, scenario := range []struct {
		name    string
		content string
		valid   bool
	}{
		{"hash", "example==1.0 --hash=sha256:abcd\n", true},
		{"continuation", "# generated lock\nexample==1.0 ; python_version >= '3.12' \\\n    --hash=sha256:abcd\n", true},
		{"include", "-r other.txt", false},
		{"wheel_override", "--no-binary :all:", false},
		{"index_override", "--index-url https://example.test", false},
		{"local_file", "example @ file:///tmp/package.whl", false},
		{"repository", "example @ git+https://example.test/project", false},
		{"trusted_host", "example==1.0 --trusted-host example.test", false},
		{"zero_byte", "example\x00==1", false},
		{"budget", strings.Repeat("x", 512*1024+1), false},
	} {
		test.Run(scenario.name, func(test *testing.T) {
			_, err := NewRequirements(reference, scenario.content)
			if (err == nil) != scenario.valid {
				test.Fatalf("requirements accepted=%t error=%v", err == nil, err)
			}
		})
	}
	first, err := NewRequirements(reference, "example==1.0 --hash=sha256:abcd\n")
	if err != nil {
		test.Fatal(err)
	}
	otherReference := reference
	otherReference.Name = "another"
	second, err := NewRequirements(otherReference, first.Content)
	if err != nil {
		test.Fatal(err)
	}
	changed, _ := NewRequirements(reference, "example==2.0 --hash=sha256:abcd\n")
	if first.Key("runtime") != second.Key("runtime") || first.Key("runtime") == first.Key("updated-runtime") || first.Key("runtime") == changed.Key("runtime") {
		test.Fatal("environment identity did not bind the runtime and complete dependency lock")
	}
	for _, index := range []string{"https://pypi.org/simple", "https://pypi.tuna.tsinghua.edu.cn/simple", "http://localhost:8080/simple"} {
		if err := ValidateIndex(&index); err != nil {
			test.Fatal(err)
		}
	}
	for _, index := range []string{"file:///tmp/wheels", "https://user:password@pypi.org/simple", "https://pypi.org/simple?token=value", "https://pypi.org/simple#fragment", "https://pypi.org/simple?", "https://pypi.org/simple#", "https://:443/simple"} {
		if ValidateIndex(&index) == nil {
			test.Fatalf("unsafe package index was accepted: %s", index)
		}
	}
	var tail stderrTail
	tail.append([]byte(strings.Repeat("a", 32*1024)))
	tail.append([]byte("retained suffix"))
	if len(tail.text()) != 16*1024 || !strings.HasSuffix(tail.text(), "retained suffix") {
		test.Fatal("preparation stderr retention was not bounded")
	}
}

func TestPythonEnvironmentLifecycle(test *testing.T) {
	test.Parallel()
	test.Run("publish_reuse_repair_and_failed_update", func(test *testing.T) {
		profile, runtimeRoot := pythonFixture(test)
		var installs atomic.Int32
		var failed atomic.Bool
		var commandsMu sync.Mutex
		var commands []Command
		now := time.Unix(1000000, 0)
		manager, err := NewManager(profile, Options{
			RuntimeDirectory: runtimeRoot,
			Now:              func() time.Time { return now },
			Run: func(ctx context.Context, command Command) error {
				commandsMu.Lock()
				commands = append(commands, command)
				commandsMu.Unlock()
				if slices.Contains(command.Arguments, "install") {
					installs.Add(1)
					if failed.Load() {
						return errors.New("fixture wheel preparation failed")
					}
				}
				return fakePreparation(command)
			},
		})
		if err != nil {
			test.Fatal(err)
		}
		index := "https://index.example.test/simple"
		if err := manager.Configure(&index); err != nil {
			test.Fatal(err)
		}
		requirements, _ := NewRequirements(protocol.PluginReference{Name: "first", Digest: strings.Repeat("a", 64)}, "example==1.0 --hash=sha256:abcd\n")
		ready, err := manager.Ensure(test.Context(), requirements)
		if err != nil {
			test.Fatal(err)
		}
		defer ready.Close()
		if ready.Environment == runtimeRoot || ready.Python != environmentInterpreter(ready.Environment) || installs.Load() != 1 {
			test.Fatal("dependency environment did not publish separately from its runtime")
		}
		again, err := manager.Ensure(test.Context(), requirements)
		if err != nil {
			test.Fatal(err)
		}
		if err := again.Close(); err != nil || installs.Load() != 1 || again.Environment != ready.Environment {
			test.Fatal("identical locked dependencies were prepared again")
		}
		if err := os.WriteFile(filepath.Join(ready.Environment, "package.py"), []byte("corrupted"), 0o600); err != nil {
			test.Fatal(err)
		}
		repaired, err := manager.Ensure(test.Context(), requirements)
		if err != nil {
			test.Fatal(err)
		}
		defer repaired.Close()
		if installs.Load() != 2 {
			test.Fatal("changed environment was reused without repair")
		}
		failed.Store(true)
		newer, _ := NewRequirements(requirements.Package, "example==2.0 --hash=sha256:abcd\n")
		if _, err := manager.Ensure(test.Context(), newer); err == nil {
			test.Fatal("failed preparation published an environment")
		}
		if data, err := os.ReadFile(filepath.Join(ready.Environment, "package.py")); err != nil || string(data) != "fixture package" {
			test.Fatal("failed new preparation changed an existing environment")
		}
		for _, command := range commands {
			if len(command.Arguments) == 0 || command.Arguments[0] != "-B" || command.Environment["PYTHONDONTWRITEBYTECODE"] != "1" || command.Environment["PYTHONNOUSERSITE"] != "1" {
				test.Fatal("preparation lost its isolated runtime environment")
			}
			if slices.Contains(command.Arguments, "install") {
				for _, required := range []string{"--isolated", "--require-hashes", "--only-binary=:all:", "--no-deps", "--no-compile", "--cache-dir", "--index-url", index} {
					if !slices.Contains(command.Arguments, required) {
						test.Fatalf("preparation omitted %s", required)
					}
				}
			}
		}
		execution, err := manager.Execution(test.Context(), protocol.NewSessionID(), []Access{{Ready: ready, Directory: "first"}, {Ready: repaired, Directory: "latest"}})
		if err != nil {
			test.Fatal(err)
		}
		if execution.Variables()["SAILRY_PYTHON"] != repaired.Python || execution.Variables()["PIP_INDEX_URL"] != index || !within(profile, execution.Session) {
			test.Fatal("command environment lost its captured Python selection")
		}
		_ = execution.Close()
		manager.Retain(nil)
		now = now.Add(8 * 24 * time.Hour)
		if err := manager.Collect(test.Context(), ""); err != nil {
			test.Fatal(err)
		}
		if _, err := os.Stat(ready.Environment); err != nil {
			test.Fatal("collection removed a captured environment lease")
		}
		_ = ready.Close()
		_ = repaired.Close()
		now = now.Add(24 * time.Hour)
		if err := manager.Collect(test.Context(), ""); err != nil {
			test.Fatal(err)
		}
		if _, err := os.Stat(ready.Environment); !errors.Is(err, os.ErrNotExist) {
			test.Fatal("idle unreferenced environment survived collection")
		}
	})
	test.Run("shared_lock_and_path_free_progress", func(test *testing.T) {
		profile, runtimeRoot := pythonFixture(test)
		entered, release, shared := make(chan struct{}), make(chan struct{}), make(chan struct{})
		var once sync.Once
		var installs atomic.Int32
		manager, err := NewManager(profile, Options{
			RuntimeDirectory: runtimeRoot,
			Run: func(ctx context.Context, command Command) error {
				if slices.Contains(command.Arguments, "install") {
					installs.Add(1)
					close(entered)
					<-release
				}
				return fakePreparation(command)
			},
			Changed: func(items []protocol.PythonPreparation) {
				if len(items) == 2 {
					body, _ := json.Marshal(items)
					if strings.Contains(string(body), profile) || strings.Contains(string(body), strings.Repeat("a", 64)) {
						test.Error("progress exposed private package paths or identities")
					}
					once.Do(func() { close(shared) })
				}
			},
		})
		if err != nil {
			test.Fatal(err)
		}
		first, _ := NewRequirements(protocol.PluginReference{Name: "first", Digest: strings.Repeat("a", 64)}, "example==1.0 --hash=sha256:abcd\n")
		second := *first
		second.Package.Name = "second"
		type result struct {
			ready *Ready
			err   error
		}
		results := make(chan result, 2)
		go func() { ready, err := manager.Ensure(test.Context(), first); results <- result{ready, err} }()
		<-entered
		go func() { ready, err := manager.Ensure(test.Context(), &second); results <- result{ready, err} }()
		<-shared
		close(release)
		for range 2 {
			current := <-results
			if current.err != nil {
				test.Fatal(current.err)
			}
			_ = current.ready.Close()
		}
		if installs.Load() != 1 || len(manager.Preparations()) != 0 {
			test.Fatal("shared preparation duplicated work or retained transient progress")
		}
	})
	test.Run("runtime_integrity_and_cancellation", func(test *testing.T) {
		_, runtimeRoot := pythonFixture(test)
		value, err := OpenRuntime(runtimeRoot)
		if err != nil {
			test.Fatal(err)
		}
		for _, relative := range []string{"__pycache__/package.cpython-312.pyc", "package.PYC", "__PYCACHE__/other.pyc"} {
			path := filepath.Join(runtimeRoot, filepath.FromSlash(relative))
			if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
				test.Fatal(err)
			}
			if err := os.WriteFile(path, []byte("unlisted bytecode"), 0o600); err != nil {
				test.Fatal(err)
			}
			if value.Verify(test.Context()) == nil {
				test.Fatal("unlisted runtime bytecode was accepted")
			}
			_ = os.Remove(path)
		}
		if err := value.Verify(test.Context()); err != nil {
			test.Fatal(err)
		}
		if runtime.GOOS != "windows" {
			environment := filepath.Join(test.TempDir(), "environment")
			if err := os.Mkdir(environment, 0o700); err != nil {
				test.Fatal(err)
			}
			link := filepath.Join(environment, "python")
			if err := os.Symlink(value.Python, link); err != nil {
				test.Fatal(err)
			}
			key := strings.Repeat("a", 64)
			if err := seal(test.Context(), environment, value.Root, key); err != nil {
				test.Fatal(err)
			}
			if valid, err := validEnvironment(test.Context(), environment, value.Root, key, hashCache{}); err != nil || !valid {
				test.Fatalf("confined runtime link=%t error=%v", valid, err)
			}
			outside := filepath.Join(test.TempDir(), "outside")
			if err := os.WriteFile(outside, []byte("outside"), 0o600); err != nil {
				test.Fatal(err)
			}
			_ = os.Remove(link)
			if err := os.Symlink(outside, link); err != nil {
				test.Fatal(err)
			}
			if valid, err := validEnvironment(test.Context(), environment, value.Root, key, hashCache{}); err == nil && valid {
				test.Fatal("outside runtime link was accepted")
			}
			if seal(test.Context(), environment, value.Root, key) == nil {
				test.Fatal("outside runtime link was sealed")
			}
		}
		if err := os.WriteFile(value.Python, []byte("changed interpreter"), 0o600); err != nil {
			test.Fatal(err)
		}
		if value.Verify(test.Context()) == nil {
			test.Fatal("changed bundled interpreter was accepted")
		}
		cancelledContext, cancel := context.WithCancel(test.Context())
		cancel()
		var fault *protocol.Fault
		err = check(cancelledContext)
		if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeCancelled {
			test.Fatal("verification lost cancellation")
		}
		if _, err := OpenRuntime(filepath.Join(test.TempDir(), "missing")); err == nil {
			test.Fatal("missing bundled runtime selected another interpreter")
		}
	})
}

func pythonFixture(test *testing.T) (string, string) {
	test.Helper()
	profile, err := filepath.EvalSymlinks(test.TempDir())
	if err != nil {
		test.Fatal(err)
	}
	root := filepath.Join(profile, "fixture-runtime")
	python := Interpreter(root)
	if err := os.MkdirAll(filepath.Dir(python), 0o700); err != nil {
		test.Fatal(err)
	}
	if err := os.WriteFile(python, []byte("fixture interpreter"), 0o600); err != nil {
		test.Fatal(err)
	}
	marker := filepath.Join(root, "lib", "python3.12", "EXTERNALLY-MANAGED")
	if runtime.GOOS == "windows" {
		marker = filepath.Join(root, "Lib", "EXTERNALLY-MANAGED")
	}
	if err := os.MkdirAll(filepath.Dir(marker), 0o700); err != nil {
		test.Fatal(err)
	}
	if err := os.WriteFile(marker, []byte("managed fixture"), 0o600); err != nil {
		test.Fatal(err)
	}
	digest, err := hashFile(test.Context(), python)
	if err != nil {
		test.Fatal(err)
	}
	relative, _ := filepath.Rel(root, python)
	receipt := runtimeReceipt{Version: 1, PythonVersion: Version, Target: "fixture", Files: []File{{Path: filepath.ToSlash(relative), Size: 19, SHA256: digest}}}
	receipt.Source.URL, receipt.Source.SHA256 = "https://example.test/python.tar.gz", strings.Repeat("a", 64)
	body, _ := json.Marshal(receipt)
	if err := os.WriteFile(filepath.Join(root, "python-runtime.json"), body, 0o600); err != nil {
		test.Fatal(err)
	}
	return profile, root
}

func fakePreparation(command Command) error {
	if slices.Contains(command.Arguments, "venv") {
		python := environmentInterpreter(command.Directory)
		if err := os.MkdirAll(filepath.Dir(python), 0o700); err != nil {
			return err
		}
		if err := os.WriteFile(python, []byte("fixture environment"), 0o600); err != nil {
			return err
		}
		return os.WriteFile(filepath.Join(command.Directory, "pyvenv.cfg"), []byte("fixture configuration"), 0o600)
	}
	if slices.Contains(command.Arguments, "install") {
		return os.WriteFile(filepath.Join(command.Directory, "package.py"), []byte("fixture package"), 0o600)
	}
	return nil
}
