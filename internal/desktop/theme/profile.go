package theme

import (
	"archive/zip"
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path"
	"path/filepath"
	"slices"
	"sort"
	"strings"
	"sync"
	"time"
)

const themeSourceFile = ".sailry-source.json"

var ErrConflict = errors.New("installed theme changed")
var ErrPublication = errors.New("theme publication could not be confirmed")

type Snapshot struct {
	Package *Package
	Files   map[string][]byte
	Digest  string
}

type Profile struct {
	Root         string
	mu           sync.Mutex
	observers    map[uint64]func()
	nextObserver uint64
}

var profiles = struct {
	sync.Mutex
	values map[string]*Profile
}{values: map[string]*Profile{}}

func NewProfile(directory string) *Profile {
	if resolved, err := filepath.EvalSymlinks(directory); err == nil {
		directory = resolved
	}
	if absolute, err := filepath.Abs(directory); err == nil {
		directory = absolute
	}
	root := filepath.Join(directory, "themes")
	profiles.Lock()
	defer profiles.Unlock()
	if profile := profiles.values[root]; profile != nil {
		return profile
	}
	profile := &Profile{Root: root, observers: map[uint64]func(){}}
	profiles.values[root] = profile
	return profile
}

func (profile *Profile) OnChange(changed func()) func() {
	profile.mu.Lock()
	profile.nextObserver++
	id := profile.nextObserver
	profile.observers[id] = changed
	profile.mu.Unlock()
	return func() { profile.mu.Lock(); delete(profile.observers, id); profile.mu.Unlock() }
}

func (profile *Profile) notify() {
	profile.mu.Lock()
	observers := make([]func(), 0, len(profile.observers))
	for _, changed := range profile.observers {
		observers = append(observers, changed)
	}
	profile.mu.Unlock()
	for _, changed := range observers {
		changed()
	}
}

func resourceNames(pkg *Package) []string {
	names := []string{"theme.json"}
	for _, variant := range []*Variant{pkg.Light, pkg.Dark} {
		if variant == nil {
			continue
		}
		for _, decoration := range variant.Assets {
			names = append(names, decoration.Path)
		}
	}
	sort.Strings(names)
	return slices.Compact(names)
}

func snapshotFiles(files map[string][]byte) (Snapshot, error) {
	pkg, err := Parse(files["theme.json"], memoryFS(files))
	if err != nil {
		return Snapshot{}, err
	}
	names := resourceNames(pkg)
	for name := range files {
		if name != themeSourceFile && !slices.Contains(names, name) {
			return Snapshot{}, errors.New("theme package contains an undeclared resource")
		}
	}
	hash := sha256.New()
	_, _ = hash.Write([]byte("Sailry theme package v1\x00"))
	for _, name := range names {
		body, exists := files[name]
		if !exists {
			return Snapshot{}, errors.New("theme resource is missing")
		}
		_, _ = hash.Write(binary.BigEndian.AppendUint64(nil, uint64(len(name))))
		_, _ = hash.Write([]byte(name))
		_, _ = hash.Write(binary.BigEndian.AppendUint64(nil, uint64(len(body))))
		_, _ = hash.Write(body)
	}
	return Snapshot{Package: pkg, Files: files, Digest: "sha256:" + hex.EncodeToString(hash.Sum(nil))}, nil
}

func ReadSnapshot(ctx context.Context, selected string) (Snapshot, error) {
	info, err := os.Lstat(selected)
	if err != nil {
		return Snapshot{}, err
	}
	if info.Mode()&os.ModeSymlink != 0 {
		return Snapshot{}, errors.New("theme source cannot be a symbolic link")
	}
	if info.Mode().IsRegular() && strings.EqualFold(filepath.Ext(selected), ".zip") {
		return readThemeArchive(ctx, selected)
	}
	directory := selected
	if info.Mode().IsRegular() && filepath.Base(selected) == "theme.json" {
		directory = filepath.Dir(selected)
	} else if !info.IsDir() {
		return Snapshot{}, errors.New("theme source must be a directory, theme.json or ZIP archive")
	}
	root, err := os.OpenRoot(directory)
	if err != nil {
		return Snapshot{}, err
	}
	defer root.Close()
	read := func(name string, limit int64) ([]byte, error) {
		for prefix := name; prefix != "."; prefix = path.Dir(prefix) {
			info, err := root.Lstat(prefix)
			if err != nil {
				return nil, err
			}
			if info.Mode()&os.ModeSymlink != 0 {
				return nil, errors.New("theme resources cannot use symbolic links")
			}
		}
		file, err := root.Open(name)
		if err != nil {
			return nil, err
		}
		defer file.Close()
		info, err := file.Stat()
		if err != nil || !info.Mode().IsRegular() || info.Size() > limit {
			return nil, errors.New("theme resource exceeds file limit")
		}
		body, err := io.ReadAll(io.LimitReader(file, limit+1))
		if len(body) > int(limit) {
			return nil, errors.New("theme resource exceeds file limit")
		}
		return body, err
	}
	manifest, err := read("theme.json", themeManifestLimit)
	if err != nil {
		return Snapshot{}, err
	}
	var declaration Package
	if err := json.Unmarshal(manifest, &declaration); err != nil {
		return Snapshot{}, err
	}
	if err := declaration.validate(); err != nil {
		return Snapshot{}, err
	}
	files := map[string][]byte{"theme.json": manifest}
	total := len(manifest)
	for _, name := range resourceNames(&declaration) {
		if ctx.Err() != nil {
			return Snapshot{}, ctx.Err()
		}
		if name == "theme.json" {
			continue
		}
		body, err := read(name, themeFileLimit)
		if err != nil {
			return Snapshot{}, err
		}
		total += len(body)
		if total > themePackageLimit {
			return Snapshot{}, errors.New("theme package exceeds resource limit")
		}
		files[name] = body
	}
	if source, err := read(themeSourceFile, themeManifestLimit); err == nil {
		files[themeSourceFile] = source
	} else if !errors.Is(err, os.ErrNotExist) {
		return Snapshot{}, err
	}
	return snapshotFiles(files)
}

func readThemeArchive(ctx context.Context, selected string) (Snapshot, error) {
	archive, err := zip.OpenReader(selected)
	if err != nil {
		return Snapshot{}, err
	}
	defer archive.Close()
	if len(archive.File) > 64 {
		return Snapshot{}, errors.New("theme archive has too many entries")
	}
	files, total := map[string][]byte{}, 0
	for _, entry := range archive.File {
		if err := ctx.Err(); err != nil {
			return Snapshot{}, err
		}
		name := entry.Name
		if entry.FileInfo().IsDir() {
			continue
		}
		if !fs.ValidPath(name) || strings.Contains(name, "\\") || !entry.Mode().IsRegular() || files[name] != nil {
			return Snapshot{}, errors.New("theme archive contains an invalid resource")
		}
		limit := themeFileLimit
		if name == "theme.json" {
			limit = themeManifestLimit
		}
		if entry.UncompressedSize64 > uint64(limit) {
			return Snapshot{}, errors.New("theme archive resource exceeds file limit")
		}
		reader, err := entry.Open()
		if err != nil {
			return Snapshot{}, err
		}
		body, readErr := io.ReadAll(io.LimitReader(reader, int64(limit+1)))
		closeErr := reader.Close()
		if readErr != nil || closeErr != nil || len(body) > limit {
			return Snapshot{}, errors.New("theme archive resource could not be read")
		}
		total += len(body)
		if total > themePackageLimit {
			return Snapshot{}, errors.New("theme archive exceeds package limit")
		}
		files[name] = body
	}
	return snapshotFiles(files)
}

func (profile *Profile) Installed(ctx context.Context) ([]Snapshot, error) {
	profile.mu.Lock()
	defer profile.mu.Unlock()
	entries, err := os.ReadDir(profile.Root)
	if errors.Is(err, os.ErrNotExist) {
		return []Snapshot{}, nil
	}
	if err != nil {
		return nil, err
	}
	var snapshots []Snapshot
	for _, entry := range entries {
		if !validThemeID(entry.Name()) {
			continue
		}
		snapshot, err := ReadSnapshot(ctx, filepath.Join(profile.Root, entry.Name()))
		if err != nil {
			return nil, fmt.Errorf("installed theme %s: %w", entry.Name(), err)
		}
		if snapshot.Package.ID != entry.Name() {
			return nil, errors.New("installed theme ID does not match its directory")
		}
		snapshots = append(snapshots, snapshot)
	}
	return snapshots, nil
}

func (profile *Profile) Install(ctx context.Context, snapshot Snapshot, expected *string) error {
	profile.mu.Lock()
	changed := false
	defer func() {
		profile.mu.Unlock()
		if changed {
			profile.notify()
		}
	}()
	if err := snapshot.Package.validate(); err != nil {
		return err
	}
	verified, err := snapshotFiles(snapshot.Files)
	if err != nil || verified.Digest != snapshot.Digest {
		return errors.New("theme snapshot changed")
	}
	if err := os.MkdirAll(profile.Root, 0700); err != nil {
		return err
	}
	info, err := os.Lstat(profile.Root)
	if err != nil || !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
		return errors.New("theme profile is not an ordinary directory")
	}
	root, err := os.OpenRoot(profile.Root)
	if err != nil {
		return err
	}
	defer root.Close()
	id := snapshot.Package.ID
	current, currentErr := ReadSnapshot(ctx, filepath.Join(profile.Root, id))
	if expected == nil && !errors.Is(currentErr, os.ErrNotExist) || expected != nil && (currentErr != nil || current.Digest != *expected) {
		return ErrConflict
	}
	stage, err := os.MkdirTemp(profile.Root, ".staging-")
	if err != nil {
		return err
	}
	defer os.RemoveAll(stage)
	for name, body := range snapshot.Files {
		if !fs.ValidPath(name) || strings.Contains(name, "\\") {
			return errors.New("theme snapshot path is invalid")
		}
		target := filepath.Join(stage, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(target), 0700); err != nil {
			return err
		}
		if err := os.WriteFile(target, body, 0600); err != nil {
			return err
		}
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	backup := ""
	if expected != nil {
		backupDirectory, err := os.MkdirTemp(profile.Root, ".replacing-")
		if err != nil {
			return err
		}
		if err := os.Remove(backupDirectory); err != nil {
			return err
		}
		backup = filepath.Base(backupDirectory)
		if err := root.Rename(id, backup); err != nil {
			return err
		}
	}
	if err := root.Rename(filepath.Base(stage), id); err != nil {
		if backup != "" {
			if restoreErr := root.Rename(backup, id); restoreErr != nil {
				return errors.Join(ErrPublication, err, restoreErr)
			}
		}
		return err
	}
	if backup != "" {
		_ = os.RemoveAll(filepath.Join(profile.Root, backup))
	}
	changed = true
	return nil
}

func (profile *Profile) Remove(ctx context.Context, id, digest string) error {
	profile.mu.Lock()
	changed := false
	defer func() {
		profile.mu.Unlock()
		if changed {
			profile.notify()
		}
	}()
	if !validThemeID(id) {
		return errors.New("invalid installed theme ID")
	}
	current, err := ReadSnapshot(ctx, filepath.Join(profile.Root, id))
	if err != nil {
		return err
	}
	if current.Digest != digest {
		return ErrConflict
	}
	root, err := os.OpenRoot(profile.Root)
	if err != nil {
		return err
	}
	defer root.Close()
	if err := ctx.Err(); err != nil {
		return err
	}
	err = root.RemoveAll(id)
	changed = err == nil
	return err
}

func (snapshot Snapshot) Export(destination string) error {
	file, err := os.OpenFile(destination, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil {
		return err
	}
	err = snapshot.WriteZIP(file)
	err = errors.Join(err, file.Sync(), file.Close())
	if err != nil {
		_ = os.Remove(destination)
	}
	return err
}

func (snapshot Snapshot) WriteZIP(destination io.Writer) error {
	archive := zip.NewWriter(destination)
	names := make([]string, 0, len(snapshot.Files))
	for name := range snapshot.Files {
		if name != themeSourceFile {
			names = append(names, name)
		}
	}
	sort.Strings(names)
	for _, name := range names {
		entry, err := archive.Create(name)
		if err != nil {
			_ = archive.Close()
			return err
		}
		if _, err := entry.Write(snapshot.Files[name]); err != nil {
			_ = archive.Close()
			return err
		}
	}
	return archive.Close()
}

type memoryFS map[string][]byte

func (files memoryFS) Open(name string) (fs.File, error) {
	if !fs.ValidPath(name) {
		return nil, fs.ErrInvalid
	}
	if body, exists := files[name]; exists {
		return &memoryFile{Reader: bytes.NewReader(body), info: memoryInfo{name: path.Base(name), size: int64(len(body))}}, nil
	}
	for candidate := range files {
		if name == "." || strings.HasPrefix(candidate, name+"/") {
			return &memoryFile{Reader: bytes.NewReader(nil), info: memoryInfo{name: path.Base(name), directory: true}}, nil
		}
	}
	return nil, fs.ErrNotExist
}

type memoryFile struct {
	*bytes.Reader
	info memoryInfo
}

func (file *memoryFile) Stat() (fs.FileInfo, error) { return file.info, nil }
func (file *memoryFile) Close() error               { return nil }

type memoryInfo struct {
	name      string
	size      int64
	directory bool
}

func (info memoryInfo) Name() string { return info.name }
func (info memoryInfo) Size() int64  { return info.size }
func (info memoryInfo) Mode() fs.FileMode {
	if info.directory {
		return fs.ModeDir | 0700
	}
	return 0600
}
func (info memoryInfo) ModTime() time.Time { return time.Time{} }
func (info memoryInfo) IsDir() bool        { return info.directory }
func (info memoryInfo) Sys() any           { return nil }
