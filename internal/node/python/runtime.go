package python

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"net/url"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync"

	"lukechampine.com/blake3"
)

const Version = "3.12.13"

type runtimeReceipt struct {
	Version       uint32 `json:"version"`
	PythonVersion string `json:"python_version"`
	Target        string `json:"target"`
	Source        struct {
		URL    string `json:"url"`
		SHA256 string `json:"sha256"`
	} `json:"source"`
	Files []File `json:"files"`
}

type Runtime struct {
	Root     string
	Python   string
	Identity string
	mu       sync.Mutex
	receipt  string
	files    []File
	cache    hashCache
}

func BundledDirectory() (string, error) {
	executable, err := os.Executable()
	if err != nil {
		return "", storage(err)
	}
	return filepath.Join(filepath.Dir(executable), "python"), nil
}

func BundledRuntime() (*Runtime, error) {
	directory, err := BundledDirectory()
	if err != nil {
		return nil, err
	}
	return OpenRuntime(directory)
}

func Interpreter(directory string) string {
	if runtime.GOOS == "windows" {
		return filepath.Join(directory, "python.exe")
	}
	return filepath.Join(directory, "bin", "python3.12")
}

func environmentInterpreter(directory string) string {
	if runtime.GOOS == "windows" {
		return filepath.Join(directory, "Scripts", "python.exe")
	}
	return filepath.Join(directory, "bin", "python")
}

func OpenRuntime(directory string) (*Runtime, error) {
	if !filepath.IsAbs(directory) {
		return nil, notConfigured()
	}
	root, err := filepath.EvalSymlinks(directory)
	if err != nil {
		return nil, notConfigured()
	}
	python, err := filepath.EvalSymlinks(Interpreter(root))
	if err != nil || !within(root, python) {
		return nil, invalid("bundled Python interpreter points outside its runtime")
	}
	info, err := os.Stat(python)
	if err != nil || !info.Mode().IsRegular() {
		return nil, notConfigured()
	}
	marker := filepath.Join(root, "lib", "python3.12", "EXTERNALLY-MANAGED")
	if runtime.GOOS == "windows" {
		marker = filepath.Join(root, "Lib", "EXTERNALLY-MANAGED")
	}
	info, err = os.Lstat(marker)
	if err != nil || !info.Mode().IsRegular() || info.Mode()&os.ModeSymlink != 0 {
		return nil, invalid("bundled Python must be externally managed")
	}
	body, err := readReceipt(filepath.Join(root, "python-runtime.json"))
	if err != nil {
		return nil, err
	}
	var receipt runtimeReceipt
	if decodeStrict(body, &receipt) != nil {
		return nil, invalid("bundled Python receipt is invalid")
	}
	source, err := url.Parse(receipt.Source.URL)
	if err != nil || source.Scheme != "https" || source.Hostname() == "" || source.User != nil || receipt.Version != 1 || receipt.PythonVersion != Version || receipt.Target == "" || !keyValid(receipt.Source.SHA256) {
		return nil, invalid("bundled Python receipt does not match its runtime")
	}
	found := false
	for _, file := range receipt.Files {
		if filepath.Join(root, filepath.FromSlash(file.Path)) == python && file.Link == nil {
			found = true
		}
	}
	if !found {
		return nil, invalid("bundled Python receipt does not match its runtime")
	}
	digest := blake3.Sum256(body)
	contentID := hex.EncodeToString(digest[:])
	encoded, err := json.Marshal([]string{contentID, root})
	if err != nil {
		return nil, storage(err)
	}
	identity := blake3.Sum256(encoded)
	value := &Runtime{Root: root, Python: python, Identity: hex.EncodeToString(identity[:]), receipt: contentID, files: receipt.Files, cache: hashCache{}}
	if err := value.Verify(context.Background()); err != nil {
		return nil, err
	}
	return value, nil
}

func (value *Runtime) Verify(ctx context.Context) error {
	value.mu.Lock()
	defer value.mu.Unlock()
	body, err := readReceipt(filepath.Join(value.Root, "python-runtime.json"))
	if err != nil {
		return unavailable("bundled Python changed; reinstall the application")
	}
	digest := blake3.Sum256(body)
	if hex.EncodeToString(digest[:]) != value.receipt {
		return unavailable("bundled Python changed; reinstall the application")
	}
	valid, err := verifyFiles(ctx, value.Root, value.Root, value.files, value.cache)
	if err != nil {
		return err
	}
	if !valid {
		return unavailable("bundled Python changed; reinstall the application")
	}
	listed := make(map[string]bool, len(value.files))
	for _, file := range value.files {
		listed[file.Path] = true
	}
	actual, err := inventory(ctx, value.Root)
	if err != nil {
		return err
	}
	for _, path := range actual {
		bytecode := strings.EqualFold(filepath.Ext(path), ".pyc")
		for _, part := range strings.Split(path, "/") {
			bytecode = bytecode || strings.EqualFold(part, "__pycache__")
		}
		if bytecode && !listed[path] {
			return unavailable("bundled Python changed; reinstall the application")
		}
	}
	return check(ctx)
}
