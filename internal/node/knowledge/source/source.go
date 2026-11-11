// Package source reads the files of a knowledge collection. A scan keeps
// the captured root open and never follows a link out of it.
package source

import (
	"context"
	"errors"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"unicode"

	"github.com/sailry/sailry-harness/internal/node/knowledge"
	"github.com/sailry/sailry-harness/internal/office"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Stat is the metadata witness that lets an unchanged file skip extraction.
type Stat struct {
	Size            uint64
	ModifiedSeconds uint64
	ModifiedNanos   uint32
	// Extractor identifies the OCR model for files it reads, zeros otherwise.
	Extractor [32]byte
}

// Part is one positioned text of a document.
type Part struct {
	Title, Text string
	Position    protocol.KnowledgePosition
}

// Document is one extracted file.
type Document struct {
	Path, Revision string
	ModifiedMs     uint64
	Stat           Stat
	Parts          []Part
}

// Entry is one scanned file: an extracted Document, a file whose stamp the
// index still holds (Cached), or a skipped path with its Reason. Excluded
// skips are protected or ignored paths and are not counted. Failed marks a
// path that could not be read now and may be readable later, unlike a skip,
// which its content decides; Directory tells that everything below it was
// not visited.
type Entry struct {
	Document  *Document
	Path      string
	Cached    bool
	Reason    string
	Excluded  bool
	Failed    bool
	Directory bool
}

// OCR reads images and scanned PDF pages. Signature identifies the selected
// model, or is zero when OCR is not configured.
type OCR struct {
	Signature func() ([32]byte, error)
	Parts     func(ctx context.Context, name string, data []byte) ([]office.Part, error)
}

func newFault(code protocol.ErrorCode, message string) error {
	return &protocol.Fault{Code: code, Message: message}
}

func invalid(message string) error { return newFault(protocol.ErrorCodeInvalidRequest, message) }

func code(err error) protocol.ErrorCode {
	var fault *protocol.Fault
	if errors.As(err, &fault) {
		return fault.Code
	}
	return protocol.ErrorCodeInternal
}

func ioFault(err error) error {
	switch {
	case errors.Is(err, fs.ErrNotExist):
		return newFault(protocol.ErrorCodeNotFound, "knowledge source does not exist")
	case errors.Is(err, fs.ErrPermission):
		return newFault(protocol.ErrorCodePermissionDenied, "knowledge source cannot be read")
	}
	return newFault(protocol.ErrorCodeUnavailable, "knowledge source is unavailable: "+err.Error())
}

func cancelled(ctx context.Context) error {
	if ctx.Err() != nil {
		return newFault(protocol.ErrorCodeCancelled, "knowledge source scan cancelled")
	}
	return nil
}

func skipped(path, reason string) *Entry { return &Entry{Path: path, Reason: reason} }

// unread reports why one entry could not be scanned: an invalid one is
// skipped, any other fault failed and may pass on a later scan.
func unread(path string, err error, directory bool) *Entry {
	reason := err.Error()
	var fault *protocol.Fault
	if errors.As(err, &fault) {
		reason = fault.Message
	}
	return &Entry{Path: path, Reason: reason, Failed: code(err) != protocol.ErrorCodeInvalidRequest, Directory: directory}
}
func excluded(path, reason string) *Entry { return &Entry{Path: path, Reason: reason, Excluded: true} }

func join(parent, name string) string {
	if parent == "" {
		return name
	}
	return parent + "/" + name
}

// components splits a normalized relative path; root allows "".
func components(relative string, root bool) ([]string, error) {
	if root && relative == "" {
		return nil, nil
	}
	if !fs.ValidPath(relative) || relative == "." || strings.ContainsAny(relative, "\\:") || strings.ContainsFunc(relative, unicode.IsControl) {
		return nil, invalid("expected a normalized worktree-relative path")
	}
	return strings.Split(relative, "/"), nil
}

func openRoot(path string) (*os.Root, error) {
	root, err := os.OpenRoot(path)
	if err != nil {
		return nil, ioFault(err)
	}
	return root, nil
}

// descend opens an ordinary child directory and closes its parent.
func descend(directory *os.Root, name string) (*os.Root, error) {
	defer directory.Close()
	metadata, err := directory.Lstat(name)
	if err != nil {
		return nil, ioFault(err)
	}
	if !metadata.IsDir() || metadata.Mode()&os.ModeSymlink != 0 {
		return nil, invalid("file parents must be ordinary directories")
	}
	child, err := directory.OpenRoot(name)
	if err != nil {
		return nil, ioFault(err)
	}
	opened, err := child.Stat(".")
	if err != nil || !os.SameFile(metadata, opened) {
		child.Close()
		if err != nil {
			return nil, ioFault(err)
		}
		return nil, newFault(protocol.ErrorCodeRevisionConflict, "knowledge source changed during inspection")
	}
	return child, nil
}

// Worktree scans a path below the root of a captured worktree.
func Worktree(ctx context.Context, root, relative string) (*Scan, error) {
	if err := cancelled(ctx); err != nil {
		return nil, err
	}
	if relative == "." {
		relative = ""
	}
	parts, err := components(relative, true)
	if err != nil {
		return nil, err
	}
	directory, err := openRoot(root)
	if err != nil {
		return nil, err
	}
	return selectScan(ctx, directory, parts, "")
}

// Controller scans a directory or file a controller registered by its
// absolute path. Locators keep the registered spelling.
func Controller(ctx context.Context, kind, path string) (*Scan, error) {
	if err := cancelled(ctx); err != nil {
		return nil, err
	}
	normalized, err := knowledge.Native(path)
	if err != nil {
		return nil, err
	}
	switch kind {
	case "directory":
		if knowledge.Protected(normalized) {
			return nil, newFault(protocol.ErrorCodePermissionDenied, "protected directories cannot be indexed")
		}
		directory, err := openRoot(normalized)
		if err != nil {
			return nil, err
		}
		return selectScan(ctx, directory, nil, path)
	case "file":
		name := filepath.Base(normalized)
		if _, err := components(name, false); err != nil {
			return nil, err
		}
		if knowledge.Protected(normalized) {
			return nil, newFault(protocol.ErrorCodePermissionDenied, "protected files cannot be indexed")
		}
		directory, err := openRoot(filepath.Dir(normalized))
		if err != nil {
			return nil, err
		}
		return fileScan(ctx, directory, name, filepath.Dir(path))
	}
	return nil, newFault(protocol.ErrorCodePermissionDenied, "controller source extraction requires a file or directory selection")
}

// Within scans a current descendant of a registered controller directory.
func Within(ctx context.Context, root, relative string) (*Scan, error) {
	if err := cancelled(ctx); err != nil {
		return nil, err
	}
	parts, err := components(relative, false)
	if err != nil {
		return nil, err
	}
	normalized, err := knowledge.Native(root)
	if err != nil {
		return nil, err
	}
	if knowledge.Protected(normalized) {
		return nil, newFault(protocol.ErrorCodePermissionDenied, "protected directories cannot be indexed")
	}
	directory, err := openRoot(normalized)
	if err != nil {
		return nil, err
	}
	return selectScan(ctx, directory, parts, root)
}
