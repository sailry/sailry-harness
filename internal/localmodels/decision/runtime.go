package decision

import (
	"crypto/sha256"
	_ "embed"
	"encoding/hex"
	"encoding/json"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"sync"

	"github.com/ebitengine/purego"
)

// lockSource pins every packaged llama.cpp file per target. It is the lock
// the packaging step stages the runtime from.
//
//go:embed llama-runtime.lock.json
var lockSource []byte

// cache keeps the first successful value; failures are retried.
type cache[T any] struct {
	mutex sync.Mutex
	value *T
}

// get reports every failure of load as Unavailable with its reason.
func (c *cache[T]) get(load func() (*T, error)) (*T, error) {
	c.mutex.Lock()
	defer c.mutex.Unlock()
	if c.value != nil {
		return c.value, nil
	}
	value, err := load()
	if err != nil {
		return nil, unavailable(reason(err))
	}
	c.value = value
	return value, nil
}

func reason(err error) string {
	if failure, typed := err.(*Error); typed && (failure.Kind == Unavailable || failure.Kind == Library) {
		return failure.Message
	}
	return err.Error()
}

var (
	// current holds the native images of this process until it exits.
	current cache[backend]
	// verified keeps the checked packaged files when a later load is retried.
	verified cache[artifact]
	// discard is the one log callback of the process; callbacks are never freed.
	discard = sync.OnceValue(func() uintptr {
		return purego.NewCallback(func(uintptr, uintptr, uintptr) uintptr { return 0 })
	})
)

// backend is the loaded pinned runtime.
type backend struct {
	api api
	// cpu is the selected CPU plugin when the registry has no built-in device.
	cpu string

	mutex       sync.Mutex
	registered  bool
	initialized bool
}

type lockFile struct {
	Version uint32                `json:"version"`
	Build   string                `json:"build"`
	Targets map[string]lockTarget `json:"targets"`
}

type lockTarget struct {
	Available    bool     `json:"available"`
	Entry        *string  `json:"entry"`
	MinimumMacOS *string  `json:"minimum_macos"`
	MinimumGlibc *string  `json:"minimum_glibc"`
	Files        []pinned `json:"files"`
}

type pinned struct {
	Name   string `json:"name"`
	Size   uint64 `json:"size"`
	SHA256 string `json:"sha256"`
}

type manifest struct {
	Version *uint32   `json:"version"`
	Target  *string   `json:"target"`
	Build   *string   `json:"build"`
	Files   *[]pinned `json:"files"`
}

type artifact struct {
	entry    string
	variants []string
}

// initialize prepares the runtime and registers its CPU backend.
func initialize() (*backend, error) {
	loaded, err := prepared()
	if err != nil {
		return nil, err
	}
	return loaded, loaded.start()
}

// prepared searches only the directory of the running executable.
func prepared() (*backend, error) {
	return current.get(func() (*backend, error) {
		executable, err := os.Executable()
		if err != nil {
			return nil, failed(err)
		}
		if executable, err = filepath.EvalSymlinks(executable); err != nil {
			return nil, failed(err)
		}
		return locate(filepath.Dir(executable), &verified)
	})
}

// locate verifies and loads the runtime packaged in one directory.
func locate(directory string, verified *cache[artifact]) (*backend, error) {
	artifact, err := verified.get(func() (*artifact, error) { return verify(directory) })
	if err != nil {
		return nil, err
	}
	return load(directory, artifact)
}

func target(system, architecture string) (string, error) {
	switch system + "/" + architecture {
	case "darwin/arm64":
		return "aarch64-apple-darwin", nil
	case "darwin/amd64":
		return "x86_64-apple-darwin", nil
	case "linux/amd64":
		return "x86_64-unknown-linux-gnu", nil
	case "linux/arm64":
		return "aarch64-unknown-linux-gnu", nil
	case "windows/arm64":
		return "aarch64-pc-windows-msvc", nil
	case "windows/amd64":
		return "x86_64-pc-windows-msvc", nil
	}
	return "", unavailable("platform has no supported prebuilt runtime")
}

func platform(asset *lockTarget) error {
	var text *string
	switch runtime.GOOS {
	case "darwin":
		text = asset.MinimumMacOS
	case "linux":
		text = asset.MinimumGlibc
	}
	var minimum *[2]uint32
	if text != nil {
		value, valid := version(*text)
		if !valid {
			return unavailable("native platform minimum is invalid")
		}
		minimum = &value
	}
	return baseline(minimum)
}

func version(value string) ([2]uint32, bool) {
	parts := strings.Split(value, ".")
	if len(parts) < 2 {
		return [2]uint32{}, false
	}
	major, errMajor := strconv.ParseUint(parts[0], 10, 32)
	minor, errMinor := strconv.ParseUint(parts[1], 10, 32)
	return [2]uint32{uint32(major), uint32(minor)}, errMajor == nil && errMinor == nil
}

func atLeast(value string, minimum [2]uint32) bool {
	current, valid := version(value)
	return valid && (current[0] > minimum[0] || current[0] == minimum[0] && current[1] >= minimum[1])
}

// ordinary resolves a path that is itself a regular file, not a link.
func ordinary(path string) (string, error) {
	info, err := os.Lstat(path)
	if err != nil {
		return "", failed(err)
	}
	if !info.Mode().IsRegular() {
		return "", unavailable("native runtime must contain ordinary files")
	}
	resolved, err := filepath.EvalSymlinks(path)
	if err != nil {
		return "", failed(err)
	}
	if resolved, err = filepath.Abs(resolved); err != nil {
		return "", failed(err)
	}
	return resolved, nil
}

func manifestPath(directory string) string {
	if contents := filepath.Dir(directory); runtime.GOOS == "darwin" &&
		filepath.Base(directory) == "MacOS" && filepath.Base(contents) == "Contents" {
		return filepath.Join(contents, "Resources", "llama-runtime.json")
	}
	return filepath.Join(directory, "llama-runtime.json")
}

// verify checks the packaged manifest and every file against the lock
// before any image is opened.
func verify(directory string) (*artifact, error) {
	identity, err := target(runtime.GOOS, runtime.GOARCH)
	if err != nil {
		return nil, err
	}
	var lock lockFile
	if json.Unmarshal(lockSource, &lock) != nil {
		return nil, unavailable("packaged runtime lock is invalid")
	}
	asset, present := lock.Targets[identity]
	if !present || !asset.Available {
		return nil, unavailable("runtime is unavailable on this platform")
	}
	if err := platform(&asset); err != nil {
		return nil, err
	}
	path, err := ordinary(manifestPath(directory))
	if err != nil {
		return nil, err
	}
	info, err := os.Stat(path)
	if err != nil {
		return nil, failed(err)
	}
	if info.Size() > 128*1024 {
		return nil, unavailable("native runtime manifest exceeds its limit")
	}
	source, err := os.ReadFile(path)
	if err != nil {
		return nil, failed(err)
	}
	var installed manifest
	if json.Unmarshal(source, &installed) != nil || installed.Version == nil || installed.Target == nil ||
		installed.Build == nil || installed.Files == nil {
		return nil, unavailable("native runtime manifest is invalid")
	}
	files := *installed.Files
	if lock.Version != 1 || *installed.Version != 1 || *installed.Target != identity ||
		*installed.Build != lock.Build || len(files) != len(asset.Files) {
		return nil, unavailable("native runtime does not match its lock")
	}
	variants := []string{}
	for index, file := range files {
		locked := asset.Files[index]
		// Signing rewrites macOS images, so there the staged manifest is the
		// record of size and digest.
		if file.Name != locked.Name || runtime.GOOS != "darwin" && (file.Size != locked.Size || file.SHA256 != locked.SHA256) {
			return nil, unavailable("native runtime provenance is invalid")
		}
		if err := checksum(filepath.Join(directory, locked.Name), file); err != nil {
			return nil, err
		}
		if variant(locked.Name) {
			variants = append(variants, locked.Name)
		}
	}
	if asset.Entry == nil {
		return nil, unavailable("native runtime entry is missing")
	}
	return &artifact{entry: *asset.Entry, variants: variants}, nil
}

func checksum(path string, file pinned) error {
	path, err := ordinary(path)
	if err != nil {
		return err
	}
	input, err := os.Open(path)
	if err != nil {
		return failed(err)
	}
	defer input.Close()
	info, err := input.Stat()
	if err != nil {
		return failed(err)
	}
	if uint64(info.Size()) != file.Size {
		return unavailable("native runtime file size does not match")
	}
	hash := sha256.New()
	if _, err := io.Copy(hash, input); err != nil {
		return failed(err)
	}
	if hex.EncodeToString(hash.Sum(nil)) != file.SHA256 {
		return unavailable("native runtime checksum does not match")
	}
	return nil
}

// names returns the base image, the backend registry and the CPU plugin of
// targets that ship exactly one.
func names() (base, registry, cpu string, err error) {
	switch {
	case runtime.GOOS == "darwin":
		return "libggml-base.0.dylib", "libggml.0.dylib", "libggml-cpu.0.dylib", nil
	case runtime.GOOS == "linux" && (runtime.GOARCH == "amd64" || runtime.GOARCH == "arm64"):
		return "libggml-base.so.0", "libggml.so.0", "", nil
	case runtime.GOOS == "windows" && runtime.GOARCH == "amd64":
		return "ggml-base.dll", "ggml.dll", "", nil
	case runtime.GOOS == "windows" && runtime.GOARCH == "arm64":
		return "ggml-base.dll", "ggml.dll", "ggml-cpu.dll", nil
	}
	return "", "", "", unavailable("CPU backend is unavailable")
}

// library opens one verified ordinary file by its absolute path.
func library(path string) (uintptr, error) {
	path, err := ordinary(path)
	if err != nil {
		return 0, err
	}
	handle, err := openLibrary(path)
	if err != nil {
		return 0, &Error{Kind: Library, Message: err.Error()}
	}
	return handle, nil
}

// load opens the images and selects the CPU plugin. Backend registration and
// initialization wait for the first model.
func load(directory string, artifact *artifact) (*backend, error) {
	baseName, registryName, cpu, err := names()
	if err != nil {
		return nil, err
	}
	base, err := library(filepath.Join(directory, baseName))
	if err != nil {
		return nil, err
	}
	registry, err := library(filepath.Join(directory, registryName))
	if err != nil {
		return nil, err
	}
	entry, err := library(filepath.Join(directory, artifact.entry))
	if err != nil {
		return nil, err
	}
	loaded := &backend{}
	if err := loaded.api.bind(base, registry, entry); err != nil {
		return nil, err
	}
	loaded.api.llamaLogSet(discard(), 0)
	loaded.api.ggmlLogSet(discard(), 0)
	if loaded.api.backendDevByType(deviceCPU) != 0 {
		return loaded, nil
	}
	var path string
	if len(artifact.variants) != 0 {
		path, err = selectCPU(directory, artifact.variants)
	} else if cpu == "" {
		err = unavailable("CPU backend variants are missing")
	} else {
		path, err = ordinary(filepath.Join(directory, cpu))
	}
	if err != nil {
		return nil, err
	}
	// The plugin and its dependencies stay loaded for the registry.
	if _, err := library(path); err != nil {
		return nil, err
	}
	if strings.ContainsRune(path, 0) {
		return nil, unavailable("CPU backend path is invalid")
	}
	loaded.cpu = path
	return loaded, nil
}

// start registers the CPU backend and initializes llama.cpp once. A step
// that succeeded is not repeated after a later step fails.
func (b *backend) start() error {
	b.mutex.Lock()
	defer b.mutex.Unlock()
	if b.initialized {
		return nil
	}
	if b.cpu != "" && !b.registered {
		if b.api.backendLoad(b.cpu) == 0 {
			return unavailable("CPU backend could not be loaded")
		}
		b.registered = true
	}
	// A registered CPU prevents the initializer's implicit directory scan.
	if b.api.backendDevByType(deviceCPU) == 0 {
		return unavailable("CPU backend has no device")
	}
	b.api.backendInit()
	b.initialized = true
	return nil
}
