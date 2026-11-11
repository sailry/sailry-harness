package onnx

import (
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime"

	"github.com/sailry/sailry-harness/internal/shipped"
)

// file is one packaged runtime file pinned by size and SHA-256.
type file struct {
	name   string
	size   int64
	sha256 string
}

// lock pins the runtime files of every packaged target. They are the
// "runtime" members of the sherpa-onnx v1.13.8 shared archives. The first
// file of a target is the library this package opens; the others are its
// load-time dependencies in the same directory.
var lock = map[string][]file{
	"darwin/arm64": {
		{"libonnxruntime.dylib", 28775120, "3567d114f7299d559993e536d605a6f46d7bc9d2542004accc80ee9bf5457f0b"},
	},
	"darwin/amd64": {
		{"libonnxruntime.dylib", 32315856, "2bf27180c09e6fd3f407c9ab25205ebf1ff95f2d53bfa35e35409b5849e0ac70"},
	},
	"linux/amd64": {
		{"libonnxruntime.so", 27026609, "4b3607aebd1784b26b6f9b20e4bd974c7ab8287043e4d095cb7d2cb40b5e566e"},
	},
	"linux/arm64": {
		{"libonnxruntime.so", 34606912, "cfc800d4438f15e8d9017521326776ca7117d8783cb847ccccc2fc61ac4d540f"},
	},
	"windows/amd64": {
		{"onnxruntime.dll", 17799168, "7f66f939a881baf4f46a2216496798edf4a1429878b646d12674aa62f27d8a25"},
		{"onnxruntime_providers_shared.dll", 104960, "551d0e1fe4c227d8542314ba718d52f4379e0c7bfe729a37c59833a884e27b4d"},
	},
	"windows/arm64": {
		{"onnxruntime.dll", 17635328, "968d081836cad9af537105f59e34d4997bca2ce6d78ff39d3e47660620e8a642"},
		{"onnxruntime_providers_shared.dll", 104448, "05fb0cc694b0671db544e77c55c3736d13be850b8b4045145e982e072e6f938f"},
	},
}

// locked lists the pinned runtime files of the running target.
func locked() ([]file, error) {
	files, packaged := lock[runtime.GOOS+"/"+runtime.GOARCH]
	if !packaged {
		return nil, errors.New("ONNX Runtime is unavailable on this platform")
	}
	return files, nil
}

// pins returns what the runtime files in directory must match: the upstream
// lock, or the package seal inside a signed application bundle, whose
// signing rewrote them.
func pins(directory string, files []file) ([]file, error) {
	seal, err := shipped.Sealed(directory)
	if err != nil {
		return nil, fmt.Errorf("ONNX Runtime is invalid: %w", err)
	}
	if seal == nil {
		return files, nil
	}
	sealed := make([]file, len(files))
	for index, pinned := range files {
		record, listed := seal.File(pinned.name)
		if !listed {
			return nil, fmt.Errorf("ONNX Runtime is invalid: %s is not in the package seal", pinned.name)
		}
		sealed[index] = file{pinned.name, record.Size, record.SHA256}
	}
	return sealed, nil
}

// Directory is the only place the runtime is loaded from: the directory of
// the running executable, or Contents/Resources of a macOS application
// bundle, where packaging puts shipped files. No search path or environment
// variable is consulted.
func Directory() (string, error) {
	executable, err := os.Executable()
	if err == nil {
		executable, err = filepath.EvalSymlinks(executable)
	}
	if err != nil {
		return "", fmt.Errorf("ONNX Runtime file is unavailable: %w", err)
	}
	directory := filepath.Dir(executable)
	if contents := filepath.Dir(directory); runtime.GOOS == "darwin" &&
		filepath.Base(directory) == "MacOS" && filepath.Base(contents) == "Contents" {
		return filepath.Join(contents, "Resources"), nil
	}
	return directory, nil
}

// ordinary reports a file that exists as itself: not a link, directory or device.
func ordinary(path string) (os.FileInfo, error) {
	info, err := os.Lstat(path)
	if err != nil {
		return nil, fmt.Errorf("ONNX Runtime file is unavailable: %w", err)
	}
	if !info.Mode().IsRegular() {
		return nil, fmt.Errorf("ONNX Runtime file is unavailable: %s is not an ordinary file", filepath.Base(path))
	}
	return info, nil
}

// Available reports whether the packaged runtime is present beside the
// executable, without reading or opening it.
func Available() error {
	files, err := locked()
	if err != nil {
		return err
	}
	directory, err := Directory()
	if err != nil {
		return err
	}
	for _, pinned := range files {
		if _, err := ordinary(filepath.Join(directory, pinned.name)); err != nil {
			return err
		}
	}
	return nil
}

// verify checks every pinned file in directory against its size and SHA-256
// and returns the path of the first one.
func verify(directory string, files []file) (string, error) {
	for _, pinned := range files {
		path := filepath.Join(directory, pinned.name)
		info, err := ordinary(path)
		if err != nil {
			return "", err
		}
		if info.Size() != pinned.size {
			return "", fmt.Errorf("ONNX Runtime is invalid: %s does not match its pinned size", pinned.name)
		}
		source, err := os.Open(path)
		if err != nil {
			return "", fmt.Errorf("ONNX Runtime file is unavailable: %w", err)
		}
		hash := sha256.New()
		_, err = io.Copy(hash, source)
		source.Close()
		if err != nil {
			return "", fmt.Errorf("ONNX Runtime file is unavailable: %w", err)
		}
		if hex.EncodeToString(hash.Sum(nil)) != pinned.sha256 {
			return "", fmt.Errorf("ONNX Runtime is invalid: %s does not match its pinned SHA-256", pinned.name)
		}
	}
	return filepath.Join(directory, files[0].name), nil
}
