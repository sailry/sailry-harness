package backup

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"sort"
	"strings"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const manifestName = "sailry-backup.json"

// The SQLite fallback's file families replace the Turso names of the reference layout.
var databaseNames = [2]string{"node.sqlite", "knowledge.sqlite"}

type File struct {
	Path   string `json:"path"`
	Size   uint64 `json:"size"`
	SHA256 string `json:"sha256"`
	Mode   uint32 `json:"mode"`
}

type Manifest struct {
	Version         uint32          `json:"version"`
	Node            protocol.NodeID `json:"node"`
	OriginalProfile string          `json:"original_profile"`
	Files           []File          `json:"files"`
}

// Snapshot is a private copy of the profile's backup content. It publishes
// regular files only: no symbolic links and no special files.
type Snapshot struct {
	Directory string
	Manifest  Manifest
	size      uint64
	paths     map[string]bool
}

func NewSnapshot(profile string, node protocol.NodeID) (*Snapshot, error) {
	temporary, err := core.Temporary(profile)
	if err != nil {
		return nil, err
	}
	directory, err := os.MkdirTemp(temporary, "backup-")
	if err != nil {
		return nil, ioFault(err)
	}
	if err := core.PrivateDirectory(filepath.Join(directory, "storage")); err != nil {
		_ = os.RemoveAll(directory)
		return nil, ioFault(err)
	}
	return &Snapshot{
		Directory: directory,
		Manifest:  Manifest{Version: 1, Node: node, OriginalProfile: profile, Files: []File{}},
		paths:     map[string]bool{},
	}, nil
}

func (snapshot *Snapshot) Close() error { return os.RemoveAll(snapshot.Directory) }

// Freeze copies one database family. The caller has checkpointed the database
// and closed every handle to it.
func (snapshot *Snapshot) Freeze(ctx context.Context, source string) error {
	name := filepath.Base(source)
	if name != databaseNames[0] && name != databaseNames[1] {
		return invalid("invalid native backup database name")
	}
	directory, err := os.OpenRoot(filepath.Dir(source))
	if err != nil {
		return ioFault(err)
	}
	defer directory.Close()
	for index, member := range core.DatabaseFiles(name) {
		if err := cancelled(ctx); err != nil {
			return err
		}
		info, err := directory.Lstat(member)
		if errors.Is(err, fs.ErrNotExist) && index > 0 {
			continue
		}
		if err != nil {
			return ioFault(err)
		}
		if !info.Mode().IsRegular() {
			return invalid("backup database must be a regular file")
		}
		if err := snapshot.copy(ctx, directory, member, "storage/"+member, 0); err != nil {
			return err
		}
	}
	return nil
}

// Capture copies the profile-relative roots the storage owner listed.
func (snapshot *Snapshot) Capture(ctx context.Context, roots []string) error {
	profile := snapshot.Manifest.OriginalProfile
	root, err := os.OpenRoot(profile)
	if err != nil {
		return ioFault(err)
	}
	defer root.Close()
	for _, path := range roots {
		managed := core.ManagedWorkspace(filepath.Join(profile, filepath.FromSlash(path)), profile)
		parts, err := components(path)
		if err != nil {
			return err
		}
		name := parts[len(parts)-1]
		parent, err := descend(root, parts[:len(parts)-1])
		if managed && errors.Is(err, fs.ErrNotExist) {
			continue
		}
		if err != nil {
			return ioFault(err)
		}
		_, err = parent.Lstat(name)
		if err == nil {
			err = snapshot.copy(ctx, parent, name, path, 0)
		} else if managed && errors.Is(err, fs.ErrNotExist) {
			err = nil
			managed = false
		} else {
			err = ioFault(err)
		}
		_ = parent.Close()
		if err != nil {
			return err
		}
		if managed {
			if err := snapshot.linked(ctx, path); err != nil {
				return err
			}
		}
	}
	return nil
}

// linked replaces a linked checkout's Git pointer with independent metadata.
func (snapshot *Snapshot) linked(ctx context.Context, path string) error {
	metadata, err := prepareGit(ctx, filepath.Join(snapshot.Manifest.OriginalProfile, filepath.FromSlash(path)), snapshot.Directory)
	if err != nil || metadata == "" {
		return err
	}
	defer os.RemoveAll(metadata)
	pointer := path + "/.git"
	index := slices.IndexFunc(snapshot.Manifest.Files, func(file File) bool { return file.Path == pointer })
	if index < 0 {
		return invalid("linked workspace Git pointer was not captured")
	}
	if err := os.Remove(filepath.Join(snapshot.Directory, filepath.FromSlash(pointer))); err != nil {
		return ioFault(err)
	}
	snapshot.size -= snapshot.Manifest.Files[index].Size
	snapshot.Manifest.Files = slices.Delete(snapshot.Manifest.Files, index, index+1)
	delete(snapshot.paths, pointer)
	directory, err := os.OpenRoot(metadata)
	if err != nil {
		return ioFault(err)
	}
	defer directory.Close()
	names, err := entries(directory)
	if err != nil {
		return err
	}
	for _, name := range names {
		if err := snapshot.copy(ctx, directory, name, pointer+"/"+name, 0); err != nil {
			return err
		}
	}
	return nil
}

func (snapshot *Snapshot) copy(ctx context.Context, source *os.Root, name, relative string, depth int) error {
	if err := cancelled(ctx); err != nil {
		return err
	}
	if depth > 64 {
		return invalid("backup source nesting exceeds the limit")
	}
	info, err := source.Lstat(name)
	if err != nil {
		return ioFault(err)
	}
	if info.Mode()&fs.ModeSymlink != 0 {
		return invalid("backup source is a symbolic link: " + relative)
	}
	if info.IsDir() {
		directory, err := source.OpenRoot(name)
		if err != nil {
			return ioFault(err)
		}
		defer directory.Close()
		names, err := entries(directory)
		if err != nil {
			return err
		}
		for _, name := range names {
			if err := snapshot.copy(ctx, directory, name, relative+"/"+name, depth+1); err != nil {
				return err
			}
		}
		return nil
	}
	if !info.Mode().IsRegular() {
		return invalid("backup source is not a regular file: " + relative)
	}
	if snapshot.paths[relative] {
		return nil
	}
	input, before, err := openRegular(source, name)
	if err != nil {
		return err
	}
	defer input.Close()
	if database(relative) {
		if err := regularDatabase(before); err != nil {
			return err
		}
	}
	if snapshot.size+uint64(before.Size()) > MaxBytes || len(snapshot.paths) >= MaxFiles {
		return invalid("backup source exceeds the export limit")
	}
	target, err := os.OpenRoot(snapshot.Directory)
	if err != nil {
		return ioFault(err)
	}
	defer target.Close()
	output, err := create(target, relative)
	if err != nil {
		return err
	}
	// The staged copy is read back into the archive and removed; only the
	// archive is flushed to disk.
	size, sum, err := copyHash(ctx, input, output, uint64(before.Size()))
	if closeErr := output.Close(); err == nil {
		err = wrapIO(closeErr)
	}
	if err != nil {
		return err
	}
	after, err := input.Stat()
	if err != nil {
		return ioFault(err)
	}
	if before.Size() != after.Size() || !before.ModTime().Equal(after.ModTime()) {
		return fault(protocol.ErrorCodeRevisionConflict, "backup source changed while reading: "+relative)
	}
	mode := uint32(0o600)
	if runtime.GOOS != "windows" && !database(relative) {
		mode |= uint32(before.Mode().Perm()) & 0o100
	}
	return snapshot.add(File{Path: relative, Size: size, SHA256: sum, Mode: mode})
}

func (snapshot *Snapshot) add(file File) error {
	if snapshot.size+file.Size > MaxBytes {
		return invalid("backup source exceeds the export limit")
	}
	if snapshot.paths[file.Path] || len(snapshot.paths) >= MaxFiles {
		return invalid("duplicate or excessive backup files")
	}
	snapshot.size += file.Size
	snapshot.paths[file.Path] = true
	snapshot.Manifest.Files = append(snapshot.Manifest.Files, file)
	return nil
}

// Packed is the snapshot's archive file inside the snapshot directory.
type Packed struct {
	Snapshot *Snapshot
	Path     string
	Size     uint64
	SHA256   string
}

// Pack writes the manifest first and then every file in path order.
func (snapshot *Snapshot) Pack(ctx context.Context) (*Packed, error) {
	sort.Slice(snapshot.Manifest.Files, func(left, right int) bool {
		return snapshot.Manifest.Files[left].Path < snapshot.Manifest.Files[right].Path
	})
	if err := snapshot.Manifest.Validate(); err != nil {
		return nil, err
	}
	manifest, err := json.Marshal(snapshot.Manifest)
	if err != nil {
		return nil, ioFault(err)
	}
	if len(manifest) > MaxManifest {
		return nil, invalid("backup manifest exceeds the limit")
	}
	file, err := os.CreateTemp(snapshot.Directory, "archive-")
	if err != nil {
		return nil, ioFault(err)
	}
	defer file.Close()
	compressed, err := gzip.NewWriterLevel(file, gzip.BestSpeed)
	if err != nil {
		return nil, ioFault(err)
	}
	archive := tar.NewWriter(compressed)
	if err := appendEntry(ctx, archive, manifestName, uint64(len(manifest)), 0o600, bytes.NewReader(manifest)); err != nil {
		return nil, err
	}
	root, err := os.OpenRoot(snapshot.Directory)
	if err != nil {
		return nil, ioFault(err)
	}
	defer root.Close()
	for _, entry := range snapshot.Manifest.Files {
		input, err := root.Open(entry.Path)
		if err != nil {
			return nil, ioFault(err)
		}
		err = appendEntry(ctx, archive, entry.Path, entry.Size, entry.Mode, input)
		_ = input.Close()
		if err != nil {
			return nil, err
		}
	}
	if err := errors.Join(archive.Close(), compressed.Close(), file.Sync()); err != nil {
		return nil, ioFault(err)
	}
	info, err := file.Stat()
	if err != nil {
		return nil, ioFault(err)
	}
	sum, err := hashFile(ctx, file.Name(), uint64(info.Size()))
	if err != nil {
		return nil, err
	}
	return &Packed{Snapshot: snapshot, Path: file.Name(), Size: uint64(info.Size()), SHA256: sum}, nil
}

func appendEntry(ctx context.Context, archive *tar.Writer, path string, size uint64, mode uint32, input io.Reader) error {
	if err := cancelled(ctx); err != nil {
		return err
	}
	header := &tar.Header{Typeflag: tar.TypeReg, Name: path, Size: int64(size), Mode: int64(mode), Format: tar.FormatGNU}
	if err := archive.WriteHeader(header); err != nil {
		return ioFault(err)
	}
	if _, _, err := copyHash(ctx, input, archive, size); err != nil {
		return err
	}
	return nil
}

// RefreshDatabases recomputes the database entries after restore validation
// checkpointed or upgraded the staged databases.
func (manifest *Manifest) RefreshDatabases(ctx context.Context, root string) error {
	directory, err := os.OpenRoot(filepath.Join(root, "storage"))
	if err != nil {
		return ioFault(err)
	}
	defer directory.Close()
	var files []File
	for _, name := range databaseNames {
		for index, member := range core.DatabaseFiles(name) {
			if err := cancelled(ctx); err != nil {
				return err
			}
			_, err := directory.Lstat(member)
			if errors.Is(err, fs.ErrNotExist) && index > 0 {
				continue
			}
			if errors.Is(err, fs.ErrNotExist) && name != databaseNames[0] {
				break
			}
			if err != nil {
				return ioFault(err)
			}
			input, info, err := openRegular(directory, member)
			if err != nil {
				return err
			}
			err = regularDatabase(info)
			var sum string
			if err == nil {
				_, sum, err = copyHash(ctx, input, io.Discard, uint64(info.Size()))
			}
			_ = input.Close()
			if err != nil {
				return err
			}
			files = append(files, File{Path: "storage/" + member, Size: uint64(info.Size()), SHA256: sum, Mode: 0o600})
		}
	}
	kept := manifest.Files[:0]
	for _, file := range manifest.Files {
		if !database(file.Path) {
			kept = append(kept, file)
		}
	}
	manifest.Files = append(kept, files...)
	sort.Slice(manifest.Files, func(left, right int) bool { return manifest.Files[left].Path < manifest.Files[right].Path })
	return manifest.Validate()
}

func (manifest *Manifest) Validate() error {
	if manifest.Version != 1 || !filepath.IsAbs(manifest.OriginalProfile) || len(manifest.Files) > MaxFiles {
		return invalid("invalid backup manifest")
	}
	var total uint64
	paths := map[string]bool{}
	for _, file := range manifest.Files {
		parts, err := components(file.Path)
		if err != nil {
			return err
		}
		valid := file.Path == "link.key" || database(file.Path) || strings.HasPrefix(file.Path, "plugins/packages/") ||
			strings.HasPrefix(file.Path, "attachments/") || strings.HasPrefix(file.Path, "artifacts/")
		for end := len(parts) - 1; end > 0 && !valid; end-- {
			valid = core.ManagedWorkspace(filepath.Join(manifest.OriginalProfile, filepath.Join(parts[:end]...)), manifest.OriginalProfile)
		}
		if !valid || paths[file.Path] || !digest(file.SHA256) || file.Mode&^0o700 != 0 {
			return invalid("invalid backup file metadata")
		}
		paths[file.Path] = true
		if total += file.Size; total > MaxBytes || total < file.Size {
			return invalid("backup content exceeds the restore limit")
		}
	}
	if !paths["storage/"+databaseNames[0]] || !paths["link.key"] {
		return invalid("backup database or identity is missing")
	}
	for _, name := range databaseNames {
		family := core.DatabaseFiles("storage/" + name)
		for _, member := range family[1:] {
			if paths[member] && !paths[family[0]] {
				return invalid("backup database or logical log is missing")
			}
		}
	}
	return nil
}

func database(path string) bool {
	for _, name := range databaseNames {
		for _, member := range core.DatabaseFiles("storage/" + name) {
			if member == path {
				return true
			}
		}
	}
	return false
}

// Unpack verifies an archive into an empty private directory.
func Unpack(ctx context.Context, input io.Reader, destination string) (Manifest, error) {
	compressed, err := gzip.NewReader(input)
	if err != nil {
		return Manifest{}, ioFault(err)
	}
	archive := tar.NewReader(compressed)
	first, err := archive.Next()
	if errors.Is(err, io.EOF) {
		return Manifest{}, invalid("backup manifest is missing")
	}
	if err != nil {
		return Manifest{}, ioFault(err)
	}
	if first.Typeflag != tar.TypeReg || first.Name != manifestName || first.Size > MaxManifest {
		return Manifest{}, invalid("backup manifest must be the first regular file")
	}
	body, err := io.ReadAll(archive)
	if err != nil {
		return Manifest{}, ioFault(err)
	}
	var manifest Manifest
	decoder := json.NewDecoder(bytes.NewReader(body))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&manifest); err != nil {
		return Manifest{}, ioFault(err)
	}
	if err := manifest.Validate(); err != nil {
		return Manifest{}, err
	}
	if err := core.PrivateDirectory(filepath.Join(destination, "storage")); err != nil {
		return Manifest{}, ioFault(err)
	}
	root, err := os.OpenRoot(destination)
	if err != nil {
		return Manifest{}, ioFault(err)
	}
	defer root.Close()
	expected := make(map[string]File, len(manifest.Files))
	for _, file := range manifest.Files {
		expected[file.Path] = file
	}
	for {
		if err := cancelled(ctx); err != nil {
			return Manifest{}, err
		}
		header, err := archive.Next()
		if errors.Is(err, io.EOF) {
			break
		}
		if err != nil {
			return Manifest{}, ioFault(err)
		}
		if header.Typeflag != tar.TypeReg {
			return Manifest{}, invalid("backup entries must be regular files")
		}
		spec, ok := expected[header.Name]
		if !ok {
			return Manifest{}, invalid("backup file is unexpected or duplicated")
		}
		delete(expected, header.Name)
		if uint64(header.Size) != spec.Size {
			return Manifest{}, invalid("backup file size differs from its manifest")
		}
		if err := place(ctx, archive, root, spec, invalid("backup file checksum differs from its manifest")); err != nil {
			return Manifest{}, err
		}
	}
	if len(expected) != 0 {
		return Manifest{}, invalid("backup contains missing files")
	}
	return manifest, nil
}

// Transfer copies verified staging into the destination's private directory.
func Transfer(ctx context.Context, source string, destination *os.Root, manifest Manifest) error {
	root, err := os.OpenRoot(source)
	if err != nil {
		return ioFault(err)
	}
	defer root.Close()
	for _, spec := range manifest.Files {
		if err := cancelled(ctx); err != nil {
			return err
		}
		input, err := root.Open(spec.Path)
		if err != nil {
			return ioFault(err)
		}
		err = place(ctx, input, destination, spec, fault(protocol.ErrorCodeRevisionConflict, "verified restore staging changed"))
		_ = input.Close()
		if err != nil {
			return err
		}
	}
	return nil
}

func place(ctx context.Context, input io.Reader, root *os.Root, spec File, mismatch error) error {
	output, err := create(root, spec.Path)
	if err != nil {
		return err
	}
	_, sum, err := copyHash(ctx, input, output, spec.Size)
	if err == nil && sum != spec.SHA256 {
		err = mismatch
	}
	if err == nil && runtime.GOOS != "windows" {
		err = wrapIO(output.Chmod(fs.FileMode(spec.Mode)))
	}
	if err == nil {
		err = wrapIO(output.Sync())
	}
	if closeErr := output.Close(); err == nil {
		err = wrapIO(closeErr)
	}
	return err
}

// components accepts slash-separated relative paths without traversal.
func components(path string) ([]string, error) {
	if path == "" || !utf8.ValidString(path) || strings.ContainsAny(path, "\\\x00") || !filepath.IsLocal(filepath.FromSlash(path)) {
		return nil, invalid("invalid backup path")
	}
	parts := strings.Split(path, "/")
	for _, part := range parts {
		if part == "" || part == "." || part == ".." {
			return nil, invalid("invalid backup path")
		}
	}
	return parts, nil
}

func entries(directory *os.Root) ([]string, error) {
	handle, err := directory.Open(".")
	if err != nil {
		return nil, ioFault(err)
	}
	defer handle.Close()
	names, err := handle.Readdirnames(-1)
	if err != nil {
		return nil, ioFault(err)
	}
	for _, name := range names {
		if _, err := components(name); err != nil {
			return nil, invalid("backup names must be UTF-8")
		}
	}
	sort.Strings(names)
	return names, nil
}

// descend opens nested directories without following symbolic links.
func descend(root *os.Root, parts []string) (*os.Root, error) {
	current, err := root.OpenRoot(".")
	if err != nil {
		return nil, err
	}
	for _, part := range parts {
		info, err := current.Lstat(part)
		if err == nil && !info.IsDir() {
			err = &fs.PathError{Op: "open", Path: part, Err: errors.New("not a directory")}
		}
		var next *os.Root
		if err == nil {
			next, err = current.OpenRoot(part)
		}
		_ = current.Close()
		if err != nil {
			return nil, err
		}
		current = next
	}
	return current, nil
}

func openRegular(root *os.Root, name string) (*os.File, fs.FileInfo, error) {
	before, err := root.Lstat(name)
	if err != nil {
		return nil, nil, ioFault(err)
	}
	if !before.Mode().IsRegular() {
		return nil, nil, invalid("backup source is not a regular file")
	}
	file, err := root.Open(name)
	if err != nil {
		return nil, nil, ioFault(err)
	}
	info, err := file.Stat()
	if err != nil || !os.SameFile(before, info) {
		_ = file.Close()
		return nil, nil, fault(protocol.ErrorCodeRevisionConflict, "backup source changed while reading")
	}
	return file, info, nil
}

// create makes private parents and a new private file; existing files are refused.
func create(root *os.Root, relative string) (*os.File, error) {
	parts, err := components(relative)
	if err != nil {
		return nil, err
	}
	directory := strings.Join(parts[:len(parts)-1], "/")
	if directory != "" {
		if err := root.MkdirAll(directory, 0o700); err != nil {
			return nil, ioFault(err)
		}
	}
	parent, err := descend(root, parts[:len(parts)-1])
	if err != nil {
		return nil, ioFault(err)
	}
	defer parent.Close()
	file, err := parent.OpenFile(parts[len(parts)-1], os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o600)
	if err != nil {
		return nil, ioFault(err)
	}
	return file, nil
}

func digest(value string) bool {
	if len(value) != 64 {
		return false
	}
	_, err := hex.DecodeString(value)
	return err == nil
}

func hashFile(ctx context.Context, path string, size uint64) (string, error) {
	file, err := os.Open(path)
	if err != nil {
		return "", ioFault(err)
	}
	defer file.Close()
	_, sum, err := copyHash(ctx, file, io.Discard, size)
	return sum, err
}

// HashFile returns the SHA-256 of an open file of a known size from its start.
func HashFile(ctx context.Context, file *os.File, size uint64) (string, error) {
	if _, err := file.Seek(0, io.SeekStart); err != nil {
		return "", ioFault(err)
	}
	_, sum, err := copyHash(ctx, file, io.Discard, size)
	if err != nil {
		return "", err
	}
	if _, err := file.Seek(0, io.SeekStart); err != nil {
		return "", ioFault(err)
	}
	return sum, nil
}

func copyHash(ctx context.Context, input io.Reader, output io.Writer, expected uint64) (uint64, string, error) {
	sum := sha256.New()
	var size uint64
	buffer := make([]byte, 256*1024)
	for {
		if err := cancelled(ctx); err != nil {
			return 0, "", err
		}
		count, err := input.Read(buffer)
		if count > 0 {
			if size += uint64(count); size > expected {
				return 0, "", invalid("backup file exceeds its declared size")
			}
			sum.Write(buffer[:count])
			if _, err := output.Write(buffer[:count]); err != nil {
				return 0, "", ioFault(err)
			}
		}
		if errors.Is(err, io.EOF) {
			break
		}
		if err != nil {
			return 0, "", ioFault(err)
		}
	}
	if size != expected {
		return 0, "", fault(protocol.ErrorCodeRevisionConflict, "backup file size changed")
	}
	return size, hex.EncodeToString(sum.Sum(nil)), nil
}

func wrapIO(err error) error {
	if err == nil {
		return nil
	}
	return ioFault(err)
}
