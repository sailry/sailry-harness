// Retained copy and move behavior derives from harbor-file-service 727ce0a
// and the reviewed Sailry file service (Apache-2.0).
package core

import (
	"context"
	"errors"
	"fmt"
	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/protocol"
	"io"
	"io/fs"
	"lukechampine.com/blake3"
	"os"
	"path/filepath"
	"slices"
	"strings"
)

func (files *Files) CopyEntry(ctx context.Context, from, to string) error {
	return files.CopyEntryTo(ctx, files, from, to, false)
}
func (files *Files) CopyEntryTo(ctx context.Context, target *Files, from, to string, moving bool) error {
	source, sourceName, e := files.parent(from, true)
	if e != nil {
		return e
	}
	defer source.Close()
	destination, targetName, e := target.parent(to, true)
	if e != nil {
		return e
	}
	defer destination.Close()
	sourceParent, e := source.Stat(".")
	if e != nil {
		return fileIOFault(e)
	}
	targetParent, e := destination.Stat(".")
	if e != nil {
		return fileIOFault(e)
	}
	original, e := source.Lstat(sourceName)
	if e != nil {
		return fileIOFault(e)
	}
	if original.IsDir() {
		sourcePath, e := filepath.EvalSymlinks(filepath.Join(files.path, filepath.FromSlash(from)))
		if e != nil {
			return fileIOFault(e)
		}
		targetPath, e := filepath.EvalSymlinks(filepath.Join(target.path, filepath.Dir(filepath.FromSlash(to))))
		if e != nil {
			return fileIOFault(e)
		}
		relative, e := filepath.Rel(sourcePath, targetPath)
		if e == nil && (relative == "." || relative != ".." && !strings.HasPrefix(relative, ".."+string(filepath.Separator))) {
			return fileFault(protocol.ErrorCodeInvalidRequest, "a directory cannot be copied into itself")
		}
	}
	if _, e = destination.Lstat(targetName); !errors.Is(e, fs.ErrNotExist) {
		if e == nil {
			return fileFault(protocol.ErrorCodeConflict, "an entry already exists at this path")
		}
		return fileIOFault(e)
	}
	temporary := ".sailry-copy-" + uuid.NewString()
	if e = destination.Mkdir(temporary, 0o700); e != nil {
		return fileIOFault(e)
	}
	stage, e := destination.OpenRoot(temporary)
	if e != nil {
		return fileIOFault(e)
	}
	defer stage.Close()
	stageIdentity, e := stage.Stat(".")
	if e != nil {
		return fileIOFault(e)
	}
	defer func() {
		if current, e := destination.Lstat(temporary); e == nil && os.SameFile(current, stageIdentity) {
			_ = destination.RemoveAll(temporary)
		}
	}()
	counter := 0
	evidence := map[string]copyEvidence{}
	_, e = copyObject(ctx, files.access, source, sourceName, stage, "entry", from, 0, &counter, moving, evidence)
	if e != nil {
		return e
	}
	staged, e := stage.Lstat("entry")
	if e != nil {
		return fileIOFault(e)
	}
	if e = files.validateEntryParent(from, sourceParent); e != nil {
		return e
	}
	if e = target.validateEntryParent(to, targetParent); e != nil {
		return e
	}
	current, e := source.Lstat(sourceName)
	if e != nil || !os.SameFile(original, current) || !fileMetadataUnchanged(original, current, original.Size()) {
		return fileConflict()
	}
	if moving {
		if e := verifyCopyEvidence(ctx, source, sourceName, from, evidence, false); e != nil {
			return fileConflict()
		}
	}
	if _, e = destination.Lstat(targetName); !errors.Is(e, fs.ErrNotExist) {
		if e == nil {
			return fileFault(protocol.ErrorCodeConflict, "an entry already exists at this path")
		}
		return fileIOFault(e)
	}
	if e = renameFileBetween(stage, "entry", destination, targetName); e != nil {
		return fileIOFault(e)
	}
	published, e := destination.Lstat(targetName)
	if e != nil || !os.SameFile(staged, published) {
		return copyUnknown()
	}
	if e = target.validateEntryParent(to, targetParent); e != nil {
		return copyUnknown()
	}
	if moving {
		if e := verifyCopyEvidence(ctx, source, sourceName, from, evidence, false); e != nil {
			return copyUnknown()
		}
		if e := verifyCopyEvidence(ctx, destination, targetName, from, evidence, true); e != nil {
			return copyUnknown()
		}
		if e = files.validateEntryParent(from, sourceParent); e != nil {
			return copyUnknown()
		}
		if e = files.Trash(from); e != nil {
			return copyUnknown()
		}
	}
	return nil
}
func copyUnknown() error {
	return fileFault(protocol.ErrorCodeOutcomeUnknown, "entry publication could not be confirmed; inspect both locations before retrying")
}
func copyObject(ctx context.Context, access FileAccess, source *os.Root, name string, target *os.Root, to, relative string, depth int, count *int, moving bool, evidence map[string]copyEvidence) (string, error) {
	*count++
	if *count > 100000 || depth > 64 {
		return "", fileFault(protocol.ErrorCodeInvalidRequest, "copy exceeds the entry or nesting limit")
	}
	if e := ctx.Err(); e != nil {
		return "", e
	}
	before, e := source.Lstat(name)
	if e != nil {
		return "", fileIOFault(e)
	}
	if before.Mode()&os.ModeSymlink != 0 || !before.IsDir() && !before.Mode().IsRegular() {
		return "", fileFault(protocol.ErrorCodeInvalidRequest, "copy requires ordinary files and directories")
	}
	hash := blake3.New(32, nil)
	children := 0
	if before.IsDir() {
		child, e := descendFiles(source, []string{name})
		if e != nil {
			return "", e
		}
		defer child.Close()
		if e = target.Mkdir(to, 0o700); e != nil {
			return "", fileIOFault(e)
		}
		next, e := target.OpenRoot(to)
		if e != nil {
			return "", fileIOFault(e)
		}
		defer next.Close()
		handle, e := child.Open(".")
		if e != nil {
			return "", e
		}
		entries, e := handle.ReadDir(100001)
		handle.Close()
		if e != nil && !errors.Is(e, io.EOF) {
			return "", fileIOFault(e)
		}
		if len(entries) > 100000 {
			return "", fileFault(protocol.ErrorCodeInvalidRequest, "copy exceeds the entry limit")
		}
		slices.SortFunc(entries, func(a, b fs.DirEntry) int { return strings.Compare(a.Name(), b.Name()) })
		for _, entry := range entries {
			childName := entry.Name()
			if !validFileEntry(childName) {
				return "", fileFault(protocol.ErrorCodeInvalidRequest, "copy contains a name not representable in the file protocol")
			}
			if !moving && strings.EqualFold(childName, ".git") {
				continue
			}
			if access == RestrictedFiles && strings.EqualFold(filepath.Base(relative), ".sailry") && strings.EqualFold(childName, "worktrees") {
				if moving {
					return "", fileFault(protocol.ErrorCodePermissionDenied, "managed worktrees cannot be moved through file operations")
				}
				continue
			}
			digest, e := copyObject(ctx, access, child, childName, next, childName, relative+"/"+childName, depth+1, count, moving, evidence)
			if e != nil {
				return "", e
			}
			fmt.Fprintf(hash, "%s:%s\x00", childName, digest)
			children++
		}
		if moving {
			if e = next.Chtimes(".", before.ModTime(), before.ModTime()); e != nil {
				return "", fileIOFault(e)
			}
		}
		if e = next.Chmod(".", before.Mode().Perm()); e != nil {
			return "", fileIOFault(e)
		}
	} else {
		input, e := openRegularFile(source, name)
		if e != nil {
			return "", e
		}
		defer input.Close()
		output, e := target.OpenFile(to, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o600)
		if e != nil {
			return "", fileIOFault(e)
		}
		defer output.Close()
		n, e := io.Copy(io.MultiWriter(output, hash), &contextReader{ctx: ctx, source: input})
		if e != nil {
			return "", fileIOFault(e)
		}
		if n != before.Size() {
			return "", fileConflict()
		}
		if moving {
			if e = target.Chtimes(to, before.ModTime(), before.ModTime()); e != nil {
				return "", fileIOFault(e)
			}
		}
		if e = output.Chmod(before.Mode().Perm()); e != nil {
			return "", fileIOFault(e)
		}
		if e = output.Sync(); e != nil {
			return "", fileIOFault(e)
		}
	}
	after, e := source.Lstat(name)
	if e != nil || !os.SameFile(before, after) || !fileMetadataUnchanged(before, after, before.Size()) {
		return "", fileConflict()
	}
	digest := fmt.Sprintf("%x", hash.Sum(nil))
	if moving {
		metadata, err := target.Lstat(to)
		if err != nil {
			return "", fileIOFault(err)
		}
		evidence[relative] = copyEvidence{source: before, target: metadata, digest: digest, children: children}
	}
	return digest, nil
}

type contextReader struct {
	ctx    context.Context
	source io.Reader
}

func (r *contextReader) Read(p []byte) (int, error) {
	if e := r.ctx.Err(); e != nil {
		return 0, e
	}
	return r.source.Read(p)
}

type copyEvidence struct {
	source, target fs.FileInfo
	digest         string
	children       int
}

func verifyCopyEvidence(ctx context.Context, root *os.Root, base, origin string, evidence map[string]copyEvidence, target bool) error {
	for relative, record := range evidence {
		if e := ctx.Err(); e != nil {
			return e
		}
		suffix := strings.TrimPrefix(relative, origin)
		name := base + suffix
		parts := strings.Split(name, "/")
		parent, e := descendFiles(root, parts[:len(parts)-1])
		if e != nil {
			return e
		}
		terminal := parts[len(parts)-1]
		expected := record.source
		if target {
			expected = record.target
		}
		current, e := parent.Lstat(terminal)
		if e != nil || !copyMetadataEqual(expected, current) {
			parent.Close()
			return fileConflict()
		}
		if current.IsDir() {
			directory, e := descendFiles(parent, []string{terminal})
			parent.Close()
			if e != nil {
				return e
			}
			handle, e := directory.Open(".")
			if e != nil {
				directory.Close()
				return e
			}
			seen := 0
			for {
				if err := ctx.Err(); err != nil {
					handle.Close()
					directory.Close()
					return err
				}
				entries, err := handle.ReadDir(128)
				for _, entry := range entries {
					seen++
					if seen > record.children {
						handle.Close()
						directory.Close()
						return fileConflict()
					}
					if _, ok := evidence[relative+"/"+entry.Name()]; !ok {
						handle.Close()
						directory.Close()
						return fileConflict()
					}
				}
				if errors.Is(err, io.EOF) {
					break
				}
				if err != nil {
					handle.Close()
					directory.Close()
					return err
				}
			}
			handle.Close()
			observed, err := directory.Stat(".")
			directory.Close()
			if err != nil || !copyMetadataEqual(expected, observed) || seen != record.children {
				return fileConflict()
			}
		} else {
			file, e := openRegularFile(parent, terminal)
			if e != nil {
				parent.Close()
				return e
			}
			metadata, e := file.Stat()
			if e != nil || !copyMetadataEqual(expected, metadata) {
				file.Close()
				parent.Close()
				return fileConflict()
			}
			hash := blake3.New(32, nil)
			n, e := io.Copy(hash, &contextReader{ctx: ctx, source: file})
			file.Close()
			after, afterErr := parent.Lstat(terminal)
			parent.Close()
			if e != nil {
				return e
			}
			if n != expected.Size() || fmt.Sprintf("%x", hash.Sum(nil)) != record.digest || afterErr != nil || !copyMetadataEqual(expected, after) {
				return fileConflict()
			}
		}
	}
	return nil
}
func copyMetadataEqual(before, after fs.FileInfo) bool {
	return before != nil && after != nil && os.SameFile(before, after) && before.Mode() == after.Mode() && before.ModTime().Equal(after.ModTime()) && before.Size() == after.Size()
}
