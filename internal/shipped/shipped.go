// Package shipped locates the files packaging puts with the executable and
// verifies native libraries against the package seal.
//
// The seal is written by `go run ./native/fetch -seal` after signing: it
// records the size and SHA-256 of each native library as shipped. Signing a
// macOS application rewrites its libraries, so inside a sealed bundle the
// seal replaces the upstream pins compiled into the loaders. The seal sits
// in Contents/Resources, where the application's code signature covers it.
// Everywhere else the loaders keep their upstream pins.
package shipped

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
)

// SealName is the seal's file name in the shipped directory.
const SealName = "native-seal.json"

// File is one sealed library.
type File struct {
	Name   string `json:"name"`
	Size   int64  `json:"size"`
	SHA256 string `json:"sha256"`
}

// Seal lists the native libraries of one package.
type Seal struct {
	Version int    `json:"version"`
	Target  string `json:"target"`
	Files   []File `json:"files"`
}

// Directory is where packaging puts shipped files: the directory of the
// running executable, or Contents/Resources of a macOS application bundle.
func Directory() (string, error) {
	directory, _, err := location()
	return directory, err
}

func location() (string, bool, error) {
	executable, err := os.Executable()
	if err == nil {
		executable, err = filepath.EvalSymlinks(executable)
	}
	if err != nil {
		return "", false, err
	}
	directory, bundle := locate(runtime.GOOS, executable)
	return directory, bundle, nil
}

func locate(goos, executable string) (string, bool) {
	directory := filepath.Dir(executable)
	if contents := filepath.Dir(directory); goos == "darwin" &&
		filepath.Base(directory) == "MacOS" && filepath.Base(contents) == "Contents" {
		return filepath.Join(contents, "Resources"), true
	}
	return directory, false
}

// Record measures the ordinary file name in directory.
func Record(directory, name string) (File, error) {
	path := filepath.Join(directory, name)
	info, err := os.Lstat(path)
	if err != nil {
		return File{}, err
	}
	if !info.Mode().IsRegular() {
		return File{}, fmt.Errorf("%s is not an ordinary file", name)
	}
	source, err := os.Open(path)
	if err != nil {
		return File{}, err
	}
	defer source.Close()
	hash := sha256.New()
	size, err := io.Copy(hash, source)
	if err != nil {
		return File{}, err
	}
	return File{Name: name, Size: size, SHA256: hex.EncodeToString(hash.Sum(nil))}, nil
}

// Read loads the seal of directory for the given GOOS/GOARCH target. A
// directory without one reports fs.ErrNotExist.
func Read(directory, target string) (*Seal, error) {
	path := filepath.Join(directory, SealName)
	info, err := os.Lstat(path)
	if err != nil {
		return nil, err
	}
	if !info.Mode().IsRegular() || info.Size() > 64*1024 {
		return nil, errors.New("package seal is invalid")
	}
	source, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	var seal Seal
	decoder := json.NewDecoder(bytes.NewReader(source))
	decoder.DisallowUnknownFields()
	if decoder.Decode(&seal) != nil || seal.Version != 1 || len(seal.Files) == 0 {
		return nil, errors.New("package seal is invalid")
	}
	if seal.Target != target {
		return nil, errors.New("package seal belongs to another platform")
	}
	seen := map[string]bool{}
	for _, file := range seal.Files {
		digest, err := hex.DecodeString(file.SHA256)
		if file.Name == "" || file.Name != filepath.Base(file.Name) || seen[file.Name] ||
			file.Size <= 0 || err != nil || len(digest) != sha256.Size {
			return nil, errors.New("package seal is invalid")
		}
		seen[file.Name] = true
	}
	return &seal, nil
}

// File returns the sealed record of name.
func (seal *Seal) File(name string) (File, bool) {
	for _, file := range seal.Files {
		if file.Name == name {
			return file, true
		}
	}
	return File{}, false
}

// Check verifies that name in directory is the ordinary file the seal
// records.
func (seal *Seal) Check(directory, name string) error {
	sealed, listed := seal.File(name)
	if !listed {
		return fmt.Errorf("%s is not in the package seal", name)
	}
	actual, err := Record(directory, name)
	if err != nil {
		return err
	}
	if actual.Size != sealed.Size || actual.SHA256 != sealed.SHA256 {
		return fmt.Errorf("%s does not match the package seal", name)
	}
	return nil
}

// Sealed returns the seal that replaces the upstream pins of the libraries
// in directory, or nil when the upstream pins apply. A seal counts only in
// Contents/Resources of the running macOS application bundle: there signing
// rewrote the libraries and the bundle's code signature covers the seal. An
// unreadable seal there is an error, never a fallback.
func Sealed(directory string) (*Seal, error) {
	own, bundle, err := location()
	if err != nil {
		return nil, err
	}
	return sealed(own, bundle, directory, runtime.GOOS+"/"+runtime.GOARCH)
}

func sealed(own string, bundle bool, directory, target string) (*Seal, error) {
	if !bundle {
		return nil, nil
	}
	resolved, err := filepath.EvalSymlinks(directory)
	if err != nil || resolved != own {
		return nil, nil
	}
	seal, err := Read(own, target)
	if errors.Is(err, fs.ErrNotExist) {
		return nil, nil
	}
	return seal, err
}
