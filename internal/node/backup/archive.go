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

	"filippo.io/age"
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

// Link is a symbolic link kept as a link. Its target is stored as written and
// is never followed, neither by the backup nor by the restore.
type Link struct {
	Path   string `json:"path"`
	Target string `json:"target"`
}

// Skipped is content the backup left out, and why.
type Skipped struct {
	Path   string `json:"path"`
	Reason string `json:"reason"`
}

// Why content is left out of a backup. The databases and the identity are
// never skipped: a backup without them fails.
const (
	skipSpecial    = "not a file, directory or symbolic link"
	skipChanged    = "changed while it was read"
	skipUnreadable = "could not be read"
	skipMissing    = "no longer exists"
	skipName       = "name cannot be stored"
	skipNesting    = "nested too deeply"
	skipLimit      = "beyond the export limit"
)

const (
	identityName = "link.key"
	// readAttempts is how often a file that changes under the copy is read
	// before it is left out.
	readAttempts = 3
	// maxSkipped bounds the names a manifest lists; SkippedTotal counts all.
	maxSkipped = 1000
	maxNesting = 64
)

type Manifest struct {
	Version         uint32          `json:"version"`
	Node            protocol.NodeID `json:"node"`
	OriginalProfile string          `json:"original_profile"`
	Files           []File          `json:"files"`
	Links           []Link          `json:"links,omitempty"`
	Skipped         []Skipped       `json:"skipped,omitempty"`
	SkippedTotal    uint32          `json:"skipped_total,omitempty"`
}

// Snapshot is a private copy of the profile's backup content: regular files,
// the symbolic links among them, and a record of what could not be copied.
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

// skip records content left out of the backup.
func (snapshot *Snapshot) skip(path, reason string) {
	snapshot.Manifest.SkippedTotal++
	if len(snapshot.Manifest.Skipped) < maxSkipped {
		snapshot.Manifest.Skipped = append(snapshot.Manifest.Skipped, Skipped{Path: strings.ToValidUTF8(path, "\uFFFD"), Reason: reason})
	}
}

// Freeze adds one database. write produces the complete database, as one
// consistent state, in a new file at the path it is given; the storage owner
// does that while the database stays in use.
func (snapshot *Snapshot) Freeze(ctx context.Context, name string, write func(ctx context.Context, target string) error) error {
	if name != databaseNames[0] && name != databaseNames[1] {
		return invalid("invalid native backup database name")
	}
	if err := cancelled(ctx); err != nil {
		return err
	}
	relative := "storage/" + name
	if err := write(ctx, filepath.Join(snapshot.Directory, "storage", name)); err != nil {
		if ctx.Err() != nil {
			return cancelled(ctx)
		}
		return ioFault(err)
	}
	directory, err := os.OpenRoot(filepath.Join(snapshot.Directory, "storage"))
	if err != nil {
		return ioFault(err)
	}
	defer directory.Close()
	for _, member := range core.DatabaseFiles(name)[1:] {
		if _, err := directory.Lstat(member); !errors.Is(err, fs.ErrNotExist) {
			return invalid("backup database copy is not one complete file")
		}
	}
	input, info, err := openRegular(directory, name)
	if err != nil {
		return err
	}
	defer input.Close()
	if err := regularDatabase(info); err != nil {
		return err
	}
	size, sum, err := copyHash(ctx, input, io.Discard, uint64(info.Size()))
	if err != nil {
		return err
	}
	return snapshot.add(File{Path: relative, Size: size, SHA256: sum, Mode: 0o600})
}

// Capture copies the profile-relative roots the storage owner listed. Only
// the identity is required: other content that is missing, unreadable or
// changing is left out and recorded.
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
		if err != nil {
			switch {
			case path == identityName:
				return ioFault(err)
			case !errors.Is(err, fs.ErrNotExist):
				snapshot.skip(path, skipUnreadable)
			case !managed:
				snapshot.skip(path, skipMissing)
			}
			continue
		}
		_, err = parent.Lstat(name)
		if managed && errors.Is(err, fs.ErrNotExist) {
			// A registered workspace whose directory was removed has no content.
			_ = parent.Close()
			continue
		}
		err = snapshot.copy(ctx, parent, name, path, 0)
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
// When the metadata cannot be read as one state the checkout keeps its
// pointer, and the backup records that its history was left out.
func (snapshot *Snapshot) linked(ctx context.Context, path string) error {
	pointer := path + "/.git"
	source := filepath.Join(snapshot.Manifest.OriginalProfile, filepath.FromSlash(path))
	var metadata string
	for attempt := 1; ; attempt++ {
		var err error
		if metadata, err = prepareGit(ctx, source, snapshot.Directory); err == nil {
			break
		}
		if code(err) == protocol.ErrorCodeCancelled {
			return err
		}
		if code(err) != protocol.ErrorCodeRevisionConflict {
			snapshot.skip(pointer, skipUnreadable)
			return nil
		}
		if attempt == readAttempts {
			snapshot.skip(pointer, skipChanged)
			return nil
		}
	}
	if metadata == "" {
		return nil
	}
	defer os.RemoveAll(metadata)
	index := slices.IndexFunc(snapshot.Manifest.Files, func(file File) bool { return file.Path == pointer })
	if index < 0 {
		// The pointer itself was left out and is already recorded.
		return nil
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

// copy adds one entry and everything below it. An error is a failure of the
// backup itself: cancellation, the snapshot's own storage, or the identity.
func (snapshot *Snapshot) copy(ctx context.Context, source *os.Root, name, relative string, depth int) error {
	if err := cancelled(ctx); err != nil {
		return err
	}
	required := relative == identityName
	info, err := source.Lstat(name)
	switch {
	case err != nil && required:
		return ioFault(err)
	case errors.Is(err, fs.ErrNotExist):
		snapshot.skip(relative, skipMissing)
		return nil
	case err != nil:
		snapshot.skip(relative, skipUnreadable)
		return nil
	case required && !info.Mode().IsRegular():
		return invalid("backup identity is not a regular file")
	case info.Mode()&fs.ModeSymlink != 0:
		target, err := source.Readlink(name)
		if err != nil || target == "" || len(target) > 4096 || !utf8.ValidString(target) || strings.ContainsRune(target, 0) {
			snapshot.skip(relative, skipUnreadable)
			return nil
		}
		if len(snapshot.paths) >= MaxFiles {
			snapshot.skip(relative, skipLimit)
			return nil
		}
		snapshot.paths[relative] = true
		snapshot.Manifest.Links = append(snapshot.Manifest.Links, Link{Path: relative, Target: target})
		return nil
	case info.IsDir():
		if depth >= maxNesting {
			snapshot.skip(relative, skipNesting)
			return nil
		}
		directory, err := source.OpenRoot(name)
		if err != nil {
			snapshot.skip(relative, skipUnreadable)
			return nil
		}
		defer directory.Close()
		handle, err := directory.Open(".")
		if err != nil {
			snapshot.skip(relative, skipUnreadable)
			return nil
		}
		names, err := handle.Readdirnames(-1)
		_ = handle.Close()
		if err != nil {
			snapshot.skip(relative, skipUnreadable)
			return nil
		}
		sort.Strings(names)
		for _, name := range names {
			if _, err := components(name); err != nil {
				snapshot.skip(relative+"/"+name, skipName)
				continue
			}
			if err := snapshot.copy(ctx, directory, name, relative+"/"+name, depth+1); err != nil {
				return err
			}
		}
		return nil
	case !info.Mode().IsRegular():
		snapshot.skip(relative, skipSpecial)
		return nil
	}
	if snapshot.paths[relative] {
		return nil
	}
	for attempt := 1; ; attempt++ {
		reason, err := snapshot.stage(ctx, source, name, relative)
		if err != nil || reason == "" {
			return err
		}
		if reason == skipChanged && attempt < readAttempts {
			continue
		}
		if required {
			return fault(protocol.ErrorCodeUnavailable, "backup identity "+reason)
		}
		snapshot.skip(relative, reason)
		return nil
	}
}

// stage copies one regular file into the snapshot and lists it. A reason
// names a source that cannot be archived as it is, and leaves nothing staged;
// an error is a failure of the backup itself.
func (snapshot *Snapshot) stage(ctx context.Context, source *os.Root, name, relative string) (reason string, err error) {
	input, before, err := openRegular(source, name)
	if err != nil {
		if code(err) == protocol.ErrorCodeRevisionConflict {
			return skipChanged, nil
		}
		return skipUnreadable, nil
	}
	defer input.Close()
	if snapshot.size+uint64(before.Size()) > MaxBytes || len(snapshot.paths) >= MaxFiles {
		return skipLimit, nil
	}
	target, err := os.OpenRoot(snapshot.Directory)
	if err != nil {
		return "", ioFault(err)
	}
	defer target.Close()
	output, err := create(target, relative)
	if err != nil {
		return "", err
	}
	// The staged copy is read back into the archive and removed; only the
	// archive is flushed to disk.
	size, sum, err := copyHash(ctx, input, output, uint64(before.Size()))
	if closeErr := output.Close(); err == nil {
		err = wrapIO(closeErr)
	}
	if err == nil {
		after, statErr := input.Stat()
		if statErr != nil {
			reason = skipUnreadable
		} else if before.Size() != after.Size() || !before.ModTime().Equal(after.ModTime()) {
			reason = skipChanged
		}
	} else {
		var unreadable *sourceError
		switch {
		case code(err) == protocol.ErrorCodeCancelled:
			_ = target.Remove(relative)
			return "", err
		case errors.As(err, &unreadable):
			reason, err = skipUnreadable, nil
		case code(err) == protocol.ErrorCodeRevisionConflict || code(err) == protocol.ErrorCodeInvalidRequest:
			// The file grew or shrank under the copy.
			reason, err = skipChanged, nil
		default:
			_ = target.Remove(relative)
			return "", err
		}
	}
	if reason != "" {
		if err := target.Remove(relative); err != nil {
			return "", ioFault(err)
		}
		return reason, nil
	}
	mode := uint32(0o600)
	if runtime.GOOS != "windows" {
		mode |= uint32(before.Mode().Perm()) & 0o100
	}
	return "", snapshot.add(File{Path: relative, Size: size, SHA256: sum, Mode: mode})
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

// Packed is the snapshot's sealed archive inside the snapshot directory.
type Packed struct {
	Snapshot *Snapshot
	Path     string
	Size     uint64
	SHA256   string
}

// Pack writes the archive straight into its password-encrypted age stream:
// the manifest first, then every file and every link in path order. The plain
// archive never reaches the disk.
func (snapshot *Snapshot) Pack(ctx context.Context, passphrase protocol.Secret) (*Packed, error) {
	if err := cancelled(ctx); err != nil {
		return nil, err
	}
	recipient, err := age.NewScryptRecipient(passphrase.Expose())
	if err != nil {
		return nil, invalid("backup encryption could not be initialized")
	}
	recipient.SetWorkFactor(workFactor)
	file, err := os.CreateTemp(snapshot.Directory, "sealed-")
	if err != nil {
		return nil, ioFault(err)
	}
	defer file.Close()
	sum := sha256.New()
	sealed, err := age.Encrypt(io.MultiWriter(file, sum), recipient)
	if err != nil {
		return nil, ioFault(err)
	}
	if err := snapshot.archive(ctx, sealed); err != nil {
		return nil, err
	}
	if err := errors.Join(sealed.Close(), file.Sync()); err != nil {
		return nil, ioFault(err)
	}
	info, err := file.Stat()
	if err != nil {
		return nil, ioFault(err)
	}
	if uint64(info.Size()) > MaxBytes {
		return nil, invalid("encrypted backup exceeds the export limit")
	}
	return &Packed{Snapshot: snapshot, Path: file.Name(), Size: uint64(info.Size()), SHA256: hex.EncodeToString(sum.Sum(nil))}, nil
}

// archive writes the plain archive: gzip over GNU tar.
func (snapshot *Snapshot) archive(ctx context.Context, output io.Writer) error {
	sort.Slice(snapshot.Manifest.Files, func(left, right int) bool {
		return snapshot.Manifest.Files[left].Path < snapshot.Manifest.Files[right].Path
	})
	sort.Slice(snapshot.Manifest.Links, func(left, right int) bool {
		return snapshot.Manifest.Links[left].Path < snapshot.Manifest.Links[right].Path
	})
	if err := snapshot.Manifest.Validate(); err != nil {
		return err
	}
	manifest, err := json.Marshal(snapshot.Manifest)
	if err != nil {
		return ioFault(err)
	}
	if len(manifest) > MaxManifest {
		return invalid("backup manifest exceeds the limit")
	}
	compressed, err := gzip.NewWriterLevel(output, gzip.BestSpeed)
	if err != nil {
		return ioFault(err)
	}
	archive := tar.NewWriter(compressed)
	if err := appendEntry(ctx, archive, manifestName, uint64(len(manifest)), 0o600, bytes.NewReader(manifest)); err != nil {
		return err
	}
	root, err := os.OpenRoot(snapshot.Directory)
	if err != nil {
		return ioFault(err)
	}
	defer root.Close()
	for _, entry := range snapshot.Manifest.Files {
		input, err := root.Open(entry.Path)
		if err != nil {
			return ioFault(err)
		}
		err = appendEntry(ctx, archive, entry.Path, entry.Size, entry.Mode, input)
		_ = input.Close()
		if err != nil {
			return err
		}
	}
	for _, link := range snapshot.Manifest.Links {
		if err := cancelled(ctx); err != nil {
			return err
		}
		header := &tar.Header{Typeflag: tar.TypeSymlink, Name: link.Path, Linkname: link.Target, Mode: 0o777, Format: tar.FormatGNU}
		if err := archive.WriteHeader(header); err != nil {
			return ioFault(err)
		}
	}
	if err := errors.Join(archive.Close(), compressed.Close()); err != nil {
		return ioFault(err)
	}
	return nil
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
	if manifest.Version != 1 || !filepath.IsAbs(manifest.OriginalProfile) || len(manifest.Files)+len(manifest.Links) > MaxFiles ||
		len(manifest.Skipped) > maxSkipped || uint64(manifest.SkippedTotal) < uint64(len(manifest.Skipped)) {
		return invalid("invalid backup manifest")
	}
	// content accepts the locations below the profile that hold user content.
	content := func(path string, parts []string) bool {
		valid := strings.HasPrefix(path, "plugins/packages/") || strings.HasPrefix(path, "attachments/") || strings.HasPrefix(path, "artifacts/")
		for end := len(parts) - 1; end > 0 && !valid; end-- {
			valid = core.ManagedWorkspace(filepath.Join(manifest.OriginalProfile, filepath.Join(parts[:end]...)), manifest.OriginalProfile)
		}
		return valid
	}
	var total uint64
	paths := map[string]bool{}
	links := map[string]bool{}
	for _, link := range manifest.Links {
		parts, err := components(link.Path)
		if err != nil {
			return err
		}
		if !content(link.Path, parts) || paths[link.Path] || link.Target == "" || len(link.Target) > 4096 ||
			!utf8.ValidString(link.Target) || strings.ContainsRune(link.Target, 0) {
			return invalid("invalid backup link metadata")
		}
		paths[link.Path], links[link.Path] = true, true
	}
	// beneath refuses an entry that would be written through a link.
	beneath := func(parts []string) bool {
		for end := len(parts) - 1; end > 0 && len(links) > 0; end-- {
			if links[strings.Join(parts[:end], "/")] {
				return true
			}
		}
		return false
	}
	for _, link := range manifest.Links {
		if beneath(strings.Split(link.Path, "/")) {
			return invalid("invalid backup link metadata")
		}
	}
	for _, file := range manifest.Files {
		parts, err := components(file.Path)
		if err != nil {
			return err
		}
		valid := file.Path == identityName || database(file.Path) || content(file.Path, parts)
		if !valid || paths[file.Path] || beneath(parts) || !digest(file.SHA256) || file.Mode&^0o700 != 0 {
			return invalid("invalid backup file metadata")
		}
		paths[file.Path] = true
		if total += file.Size; total > MaxBytes || total < file.Size {
			return invalid("backup content exceeds the restore limit")
		}
	}
	for _, skipped := range manifest.Skipped {
		if len(skipped.Path) > 2*4096 || len(skipped.Reason) > 256 || !utf8.ValidString(skipped.Path) || !utf8.ValidString(skipped.Reason) {
			return invalid("invalid backup skip record")
		}
	}
	if !paths["storage/"+databaseNames[0]] || !paths[identityName] {
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

// Unpack verifies an archive into an empty private directory. Links are
// checked against the manifest here and created by Transfer, in the profile
// they belong to. A stream that does not read as an archive is reported as
// damaged, apart from a failure of the destination.
func Unpack(ctx context.Context, input io.Reader, destination string) (Manifest, error) {
	compressed, err := gzip.NewReader(input)
	if err != nil {
		return Manifest{}, damaged(ctx)
	}
	archive := tar.NewReader(compressed)
	first, err := archive.Next()
	if errors.Is(err, io.EOF) {
		return Manifest{}, invalid("backup manifest is missing")
	}
	if err != nil {
		return Manifest{}, damaged(ctx)
	}
	if first.Typeflag != tar.TypeReg || first.Name != manifestName || first.Size > MaxManifest {
		return Manifest{}, invalid("backup manifest must be the first regular file")
	}
	body, err := io.ReadAll(archive)
	if err != nil {
		return Manifest{}, damaged(ctx)
	}
	var manifest Manifest
	decoder := json.NewDecoder(bytes.NewReader(body))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&manifest); err != nil {
		return Manifest{}, invalid("backup manifest is not readable by this version")
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
	links := make(map[string]string, len(manifest.Links))
	for _, link := range manifest.Links {
		links[link.Path] = link.Target
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
			return Manifest{}, damaged(ctx)
		}
		if header.Typeflag == tar.TypeSymlink {
			if target, ok := links[header.Name]; !ok || target != header.Linkname {
				return Manifest{}, invalid("backup link is unexpected or differs from its manifest")
			}
			delete(links, header.Name)
			continue
		}
		if header.Typeflag != tar.TypeReg {
			return Manifest{}, invalid("backup entries must be regular files or symbolic links")
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
			var unreadable *sourceError
			if errors.As(err, &unreadable) {
				return Manifest{}, damaged(ctx)
			}
			return Manifest{}, err
		}
	}
	if len(expected) != 0 || len(links) != 0 {
		return Manifest{}, invalid("backup contains missing files")
	}
	// The gzip trailer and the end of the encrypted stream are checked only
	// when the input is read to its end.
	if _, err := io.Copy(io.Discard, compressed); err != nil {
		return Manifest{}, damaged(ctx)
	}
	return manifest, nil
}

// damaged reports an archive stream that does not read back.
func damaged(ctx context.Context) error {
	if err := cancelled(ctx); err != nil {
		return err
	}
	return invalid("backup archive is damaged or incomplete")
}

// Transfer copies verified staging into the destination's private directory
// and then creates the links, so that nothing is ever written through one.
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
	for _, link := range manifest.Links {
		if err := cancelled(ctx); err != nil {
			return err
		}
		parts := strings.Split(link.Path, "/")
		if directory := strings.Join(parts[:len(parts)-1], "/"); directory != "" {
			if err := destination.MkdirAll(directory, 0o700); err != nil {
				return ioFault(err)
			}
		}
		parent, err := descend(destination, parts[:len(parts)-1])
		if err != nil {
			return ioFault(err)
		}
		err = parent.Symlink(link.Target, parts[len(parts)-1])
		_ = parent.Close()
		if err != nil {
			return fault(protocol.ErrorCodeUnavailable, "restore could not create the symbolic link "+link.Path+": "+err.Error())
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
			return 0, "", &sourceError{cause: ioFault(err)}
		}
	}
	if size != expected {
		return 0, "", fault(protocol.ErrorCodeRevisionConflict, "backup file size changed")
	}
	return size, hex.EncodeToString(sum.Sum(nil)), nil
}

// sourceError is a failure to read the input of a copy, as opposed to a
// failure to write its output.
type sourceError struct{ cause error }

func (failure *sourceError) Error() string { return failure.cause.Error() }
func (failure *sourceError) Unwrap() error { return failure.cause }

func wrapIO(err error) error {
	if err == nil {
		return nil
	}
	return ioFault(err)
}
