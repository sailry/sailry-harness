// File acquisition and rename retain Apache-2.0 adaptations from
// sailry-code 67ae9fa0 and harbor-file-service 727ce0a.

package core

import (
	"context"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path"
	"path/filepath"
	"runtime"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type FileAccess uint8

const (
	RestrictedFiles FileAccess = iota
	ManualFiles
)

// FileAccessForRequest derives controller access from the admitted caller.
func FileAccessForRequest(request protocol.Request, internal bool) FileAccess {
	if !internal && (request.Plugin == nil || request.Plugin.Turn == nil && request.Plugin.Invocation == nil) {
		return ManualFiles
	}
	return RestrictedFiles
}

// Files keeps the registered directory open for every confined operation.
type Files struct {
	root          *os.Root
	path          string
	access        FileAccess
	globalConfigs []string
	defaultIgnore string
	renameHook    func(bool)
}

func OpenFiles(directory string, access FileAccess) (*Files, error) {
	root, err := acquireFileRoot(directory)
	if err != nil {
		return nil, err
	}
	configs, defaultIgnore := fileGitConfigSources()
	return &Files{root: root, path: directory, access: access, globalConfigs: configs, defaultIgnore: defaultIgnore}, nil
}

func acquireFileRoot(directory string) (*os.Root, error) {
	if !filepath.IsAbs(directory) {
		return nil, fileFault(protocol.ErrorCodeInvalidRequest, "an absolute worktree directory is required")
	}
	volume := filepath.VolumeName(directory)
	base, err := os.OpenRoot(volume + string(filepath.Separator))
	if err != nil {
		return nil, fileIOFault(err)
	}
	defer base.Close()
	var parts []string
	for _, part := range strings.Split(filepath.ToSlash(directory[len(volume):]), "/") {
		switch part {
		case "", ".":
			continue
		case "..":
			return nil, fileFault(protocol.ErrorCodeInvalidRequest, "expected an absolute worktree directory without parent components")
		default:
			parts = append(parts, part)
		}
	}
	return descendFiles(base, parts)
}

func (files *Files) Close() error { return files.root.Close() }

func fileComponents(value string, allowRoot bool, access FileAccess, mutation bool) ([]string, error) {
	if allowRoot && (value == "" || value == ".") {
		return nil, nil
	}
	// ValidPath enforces the wire's normalized relative-name contract. Root owns
	// filesystem confinement, including links and concurrent directory changes.
	if !fs.ValidPath(value) || value == "." || strings.ContainsAny(value, "\\:") || strings.ContainsFunc(value, unicode.IsControl) {
		return nil, fileFault(protocol.ErrorCodeInvalidRequest, "expected a normalized worktree-relative path")
	}
	parts := strings.Split(value, "/")
	if runtime.GOOS == "windows" {
		for _, part := range parts {
			if !windowsFileComponent(part) {
				return nil, fileFault(protocol.ErrorCodeInvalidRequest, "expected a normalized worktree-relative path")
			}
		}
	}
	if access == RestrictedFiles && len(parts) >= 2 && managedFileDirectory(parts[0], parts[1]) {
		return nil, fileFault(protocol.ErrorCodePermissionDenied, "managed worktrees require their own file scope")
	}
	if mutation {
		for _, part := range parts {
			if strings.EqualFold(part, ".git") {
				return nil, fileFault(protocol.ErrorCodePermissionDenied, "Git metadata cannot be changed through file operations")
			}
		}
	}
	return parts, nil
}

func managedFileDirectory(parent, name string) bool {
	return len(parent) == len(".sailry") && len(name) == len("worktrees") &&
		strings.EqualFold(parent, ".sailry") && strings.EqualFold(name, "worktrees")
}

func (files *Files) parent(relative string, mutation bool) (*os.Root, string, error) {
	parts, err := fileComponents(relative, false, files.access, mutation)
	if err != nil {
		return nil, "", err
	}
	parent, err := descendFiles(files.root, parts[:len(parts)-1])
	if err != nil {
		return nil, "", err
	}
	return parent, parts[len(parts)-1], nil
}

func descendFiles(root *os.Root, parts []string) (*os.Root, error) {
	current, err := root.OpenRoot(".")
	if err != nil {
		return nil, fileIOFault(err)
	}
	for _, part := range parts {
		metadata, err := current.Lstat(part)
		if err != nil {
			current.Close()
			return nil, fileIOFault(err)
		}
		if !metadata.IsDir() || metadata.Mode()&os.ModeSymlink != 0 {
			current.Close()
			return nil, fileFault(protocol.ErrorCodeInvalidRequest, "file parents must be ordinary directories")
		}
		child, err := current.OpenRoot(part)
		current.Close()
		if err != nil {
			return nil, fileIOFault(err)
		}
		opened, err := child.Stat(".")
		if err != nil || !os.SameFile(metadata, opened) {
			child.Close()
			if err != nil {
				return nil, fileIOFault(err)
			}
			return nil, fileConflict()
		}
		current = child
	}
	return current, nil
}

func openRegularFile(root *os.Root, name string) (*os.File, error) {
	metadata, err := root.Lstat(name)
	if err != nil {
		return nil, fileIOFault(err)
	}
	if !metadata.Mode().IsRegular() {
		return nil, fileFault(protocol.ErrorCodeInvalidRequest, "only regular files can be read")
	}
	file, err := root.OpenFile(name, regularFileFlags(), 0)
	if err != nil {
		return nil, fileIOFault(err)
	}
	if _, err := fileNativeIdentity(file); err != nil {
		file.Close()
		return nil, err
	}
	metadata, err = file.Stat()
	if err != nil || !metadata.Mode().IsRegular() {
		file.Close()
		if err != nil {
			return nil, fileIOFault(err)
		}
		return nil, fileFault(protocol.ErrorCodeInvalidRequest, "only regular files can be read")
	}
	return file, nil
}

func (files *Files) Open(relative string) (*os.File, error) {
	parent, name, err := files.parent(relative, false)
	if err != nil {
		return nil, missingFileFault(err)
	}
	defer parent.Close()
	file, err := openRegularFile(parent, name)
	return file, missingFileFault(err)
}

func (files *Files) CreateDirectory(relative string) error {
	parent, name, err := files.parent(relative, true)
	if err != nil {
		return err
	}
	defer parent.Close()
	if err := parent.Mkdir(name, 0o777); err != nil {
		if !errors.Is(err, fs.ErrExist) {
			return fileIOFault(err)
		}
		metadata, err := parent.Lstat(name)
		if err != nil {
			return fileIOFault(err)
		}
		if !metadata.IsDir() || metadata.Mode()&os.ModeSymlink != 0 {
			return fileFault(protocol.ErrorCodeConflict, "an entry already exists at this path")
		}
	}
	return nil
}

func (files *Files) createParents(relative string) error {
	parts, err := fileComponents(relative, false, files.access, true)
	if err != nil {
		return err
	}
	current, err := files.root.OpenRoot(".")
	if err != nil {
		return fileIOFault(err)
	}
	defer func() { current.Close() }()
	for _, name := range parts[:len(parts)-1] {
		if err := current.Mkdir(name, 0o777); err != nil && !errors.Is(err, fs.ErrExist) {
			return fileIOFault(err)
		}
		child, err := descendFiles(current, []string{name})
		if err != nil {
			return err
		}
		current.Close()
		current = child
	}
	return nil
}

// Rename moves an entry after the caller has admitted its mutation.
func (files *Files) Rename(from, to string) error {
	source, sourceName, err := files.parent(from, true)
	if err != nil {
		return err
	}
	defer source.Close()
	target, targetName, err := files.parent(to, true)
	if err != nil {
		return err
	}
	defer target.Close()
	sourceID, err := source.Stat(".")
	if err != nil {
		return fileIOFault(err)
	}
	targetID, err := target.Stat(".")
	if err != nil {
		return fileIOFault(err)
	}
	if _, err := source.Lstat(sourceName); err != nil {
		return fileIOFault(err)
	}
	if sourceName == targetName && os.SameFile(sourceID, targetID) {
		return nil
	}
	if files.renameHook != nil {
		files.renameHook(false)
	}
	if err := files.validateEntryParent(from, sourceID); err != nil {
		return err
	}
	if err := files.validateEntryParent(to, targetID); err != nil {
		return err
	}
	if _, err := target.Lstat(targetName); err == nil {
		return fileFault(protocol.ErrorCodeConflict, "an entry already exists at this path")
	} else if !errors.Is(err, fs.ErrNotExist) {
		return fileIOFault(err)
	}
	if err := renameFileBetween(source, sourceName, target, targetName); err != nil {
		return fileIOFault(err)
	}
	if files.renameHook != nil {
		files.renameHook(true)
	}
	for _, parent := range []struct {
		relative string
		identity fs.FileInfo
	}{{from, sourceID}, {to, targetID}} {
		if err := files.validateEntryParent(parent.relative, parent.identity); err != nil {
			return fileFault(protocol.ErrorCodeOutcomeUnknown, "entry move could not be confirmed; inspect both locations before retrying")
		}
	}
	return nil
}

func (files *Files) validateEntryParent(relative string, expected fs.FileInfo) error {
	current, err := acquireFileRoot(files.path)
	if err != nil {
		return err
	}
	defer current.Close()
	parts, err := fileComponents(relative, false, files.access, true)
	if err != nil {
		return err
	}
	parent, err := descendFiles(current, parts[:len(parts)-1])
	if err != nil {
		return err
	}
	defer parent.Close()
	identity, err := parent.Stat(".")
	if err != nil {
		return fileIOFault(err)
	}
	if !os.SameFile(identity, expected) {
		return fileFault(protocol.ErrorCodeRevisionConflict, "entry parent changed")
	}
	return nil
}

type fileControl struct {
	context  context.Context
	deadline time.Time
}

func newFileControl(ctx context.Context) fileControl {
	return fileControl{context: ctx, deadline: time.Now().Add(5 * time.Second)}
}

func (control fileControl) check() error {
	if err := control.context.Err(); err != nil {
		if errors.Is(err, context.DeadlineExceeded) {
			return fileFault(protocol.ErrorCodeBusy, "file read deadline exceeded")
		}
		return fileFault(protocol.ErrorCodeCancelled, "file read cancelled")
	}
	if !control.deadline.IsZero() && !time.Now().Before(control.deadline) {
		return fileFault(protocol.ErrorCodeBusy, "file read deadline exceeded")
	}
	return nil
}

func readFileBytes(reader io.Reader, limit int, control fileControl) ([]byte, error) {
	data := make([]byte, 0, min(limit, 8192))
	var buffer [8192]byte
	for len(data) < limit {
		if err := control.check(); err != nil {
			return nil, err
		}
		count, err := reader.Read(buffer[:min(len(buffer), limit-len(data))])
		data = append(data, buffer[:count]...)
		if err != nil {
			if errors.Is(err, io.EOF) {
				break
			}
			return nil, fileIOFault(err)
		}
	}
	return data, control.check()
}

func fileFault(code protocol.ErrorCode, message string) *protocol.Fault {
	return &protocol.Fault{Code: code, Message: message}
}

func fileIOFault(err error) error {
	if err == nil {
		return nil
	}
	code := protocol.ErrorCodeInvalidRequest
	if errors.Is(err, fs.ErrNotExist) {
		code = protocol.ErrorCodeNotFound
	} else if errors.Is(err, fs.ErrPermission) {
		code = protocol.ErrorCodePermissionDenied
	}
	return fileFault(code, fmt.Sprintf("worktree filesystem operation failed: %v", err))
}

func missingFileFault(err error) error {
	var fault *protocol.Fault
	if errors.As(err, &fault) && fault.Code == protocol.ErrorCodeNotFound && fault.Details == nil {
		copy := *fault
		copy.Details = &protocol.FaultDetails{Kind: "worktree_path_not_found"}
		return &copy
	}
	return err
}

func fileConflict() error {
	return fileFault(protocol.ErrorCodeRevisionConflict, "file changed outside this operation")
}

func fileUnknown() error {
	return fileFault(protocol.ErrorCodeOutcomeUnknown, "file save could not be confirmed; inspect the file before retrying")
}

func fileMetadataUnchanged(before, after fs.FileInfo, size int64) bool {
	return before.Size() == size && before.Size() == after.Size() && before.ModTime().Equal(after.ModTime())
}

func fileSearchRelative(parent, name string) string {
	if parent == "" {
		return name
	}
	return parent + "/" + name
}

func fileParentPath(relative string) string {
	parent := path.Dir(relative)
	if parent == "." {
		return ""
	}
	return parent
}

func validFileEntry(name string) bool {
	return utf8.ValidString(name) && fs.ValidPath(name) && !strings.ContainsAny(name, "\\:") && !strings.ContainsFunc(name, unicode.IsControl) && (runtime.GOOS != "windows" || windowsFileComponent(name))
}

func windowsFileComponent(name string) bool {
	if strings.HasSuffix(name, ".") || strings.HasSuffix(name, " ") || strings.ContainsAny(name, "<>\"|?*") {
		return false
	}
	stem, _, _ := strings.Cut(name, ".")
	stem = strings.Map(func(character rune) rune {
		if character >= 'a' && character <= 'z' {
			return character - 'a' + 'A'
		}
		return character
	}, strings.TrimRight(stem, " "))
	switch stem {
	case "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$":
		return false
	}
	for _, prefix := range []string{"COM", "LPT"} {
		if suffix, ok := strings.CutPrefix(stem, prefix); ok {
			switch suffix {
			case "1", "2", "3", "4", "5", "6", "7", "8", "9", "¹", "²", "³":
				return false
			}
		}
	}
	return true
}
