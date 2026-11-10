package assets

import (
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"io"
	"os"
	"path/filepath"
	"slices"
	"time"
)

// receipt binds a validation marker to the file identities whose contents
// were hashed.
const receipt = ".verified"

// Stamp is the identity of a model directory and its files at one instant.
type Stamp struct {
	path      string
	files     []identity
	directory native
}

type identity struct {
	bytes    uint64
	modified time.Time
	native   native
}

// Equal reports whether both stamps observed the same files.
func (stamp *Stamp) Equal(other *Stamp) bool {
	if stamp == nil || other == nil {
		return stamp == other
	}
	return stamp.path == other.path && stamp.directory == other.directory && slices.EqualFunc(stamp.files, other.files, func(left, right identity) bool {
		return left.bytes == right.bytes && left.modified.Equal(right.modified) && left.native == right.native
	})
}

func (stamp *Stamp) digest(marker string) string {
	hash := sha256.New()
	hash.Write([]byte(marker))
	hash.Write([]byte{0})
	hash.Write([]byte(stamp.path))
	hash.Write([]byte{0})
	stamp.directory.write(hash, true)
	for _, file := range stamp.files {
		_ = binary.Write(hash, binary.LittleEndian, file.bytes)
		_ = binary.Write(hash, binary.LittleEndian, file.modified.UnixNano())
		file.native.write(hash, false)
	}
	return hex.EncodeToString(hash.Sum(nil))
}

// Candidate observes the directory without hashing file contents.
func Candidate(path string, files []File) *Stamp {
	if _, err := validate(files); err != nil {
		return nil
	}
	root, err := open(path, false)
	if err != nil {
		return nil
	}
	defer root.Close()
	directory, err := os.Lstat(path)
	if err != nil || !directory.IsDir() {
		return nil
	}
	if resolved, err := filepath.EvalSymlinks(path); err != nil || resolved != path {
		return nil
	}
	identities := make([]identity, 0, len(files))
	for _, file := range files {
		info, err := root.Lstat(file.Name)
		if err != nil || !info.Mode().IsRegular() || uint64(info.Size()) != file.Size {
			return nil
		}
		identities = append(identities, identity{bytes: uint64(info.Size()), modified: info.ModTime(), native: nativeOf(info)})
	}
	return &Stamp{path: path, files: identities, directory: nativeOf(directory)}
}

// StampOf observes the directory when it carries the expected marker.
func StampOf(path string, files []File, marker string) *Stamp {
	if !marked(path, marker) {
		return nil
	}
	return Candidate(path, files)
}

// Verified hashes every file and records a receipt so unchanged files are not
// hashed again.
func Verified(path string, files []File, marker string) *Stamp {
	before := StampOf(path, files, marker)
	if before == nil || !Ready(path, files) || !before.Equal(StampOf(path, files, marker)) {
		return nil
	}
	// A read-only profile keeps its verified files and hashes them again next start.
	_ = record(path, marker, before)
	return before
}

// Recorded reports whether a receipt proves these marked files were already
// hashed.
func Recorded(path, marker string, stamp *Stamp) bool {
	digest := stamp.digest(marker)
	return read(path, receipt, len(digest)) == digest
}

func record(path, marker string, stamp *Stamp) error {
	root, err := open(path, false)
	if err != nil {
		return err
	}
	defer root.Close()
	return replace(root, receipt, []byte(stamp.digest(marker)))
}

func marked(path, expected string) bool {
	return expected != "" && read(path, "validated", len(expected)) == expected
}

func read(path, name string, length int) string {
	root, err := open(path, false)
	if err != nil {
		return ""
	}
	defer root.Close()
	if info, err := root.Lstat(name); err != nil || !info.Mode().IsRegular() || info.Size() != int64(length) {
		return ""
	}
	file, err := root.Open(name)
	if err != nil {
		return ""
	}
	defer file.Close()
	body, err := io.ReadAll(io.LimitReader(file, int64(length)+1))
	if err != nil {
		return ""
	}
	return string(body)
}

// WriteMarker names the catalog entry whose validation the directory passed.
func WriteMarker(path, marker string) error {
	if marker == "" || len(marker) > 1024 {
		return ErrInvalidAsset
	}
	root, err := open(path, false)
	if err != nil {
		return err
	}
	defer root.Close()
	return replace(root, "validated", []byte(marker))
}

// Mark records that the files observed by expected passed validation.
func Mark(path string, files []File, marker string, expected *Stamp) (*Stamp, error) {
	if !Ready(path, files) || !expected.Equal(Candidate(path, files)) || expected == nil {
		return nil, ErrInvalidAsset
	}
	// The receipt precedes the marker, so observers never see marked files without it.
	if err := record(path, marker, expected); err != nil {
		return nil, err
	}
	if err := WriteMarker(path, marker); err != nil {
		return nil, err
	}
	actual := StampOf(path, files, marker)
	if !expected.Equal(actual) {
		return nil, ErrInvalidAsset
	}
	return actual, nil
}
