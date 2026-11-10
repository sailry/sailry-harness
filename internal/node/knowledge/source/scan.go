package source

import (
	"context"
	"errors"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"unicode/utf8"

	"github.com/go-git/go-git/v5/plumbing/format/gitignore"

	"github.com/sailry/sailry-harness/internal/node/knowledge"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const ignoreLimit = 1024 * 1024

// layer is the retained .gitignore of one directory.
type layer []gitignore.Pattern

// loadIgnore parses only the directory's own .gitignore; ambient ignore
// discovery could follow external links.
func loadIgnore(ctx context.Context, directory *os.Root, relative string) (layer, error) {
	if err := cancelled(ctx); err != nil {
		return nil, err
	}
	metadata, err := directory.Lstat(".gitignore")
	if errors.Is(err, fs.ErrNotExist) {
		return nil, nil
	}
	if err != nil {
		return nil, ioFault(err)
	}
	if !metadata.Mode().IsRegular() || metadata.Size() > ignoreLimit {
		return nil, invalid(".gitignore must be a regular file within 1 MiB")
	}
	file, err := directory.Open(".gitignore")
	if err != nil {
		return nil, ioFault(err)
	}
	data, err := io.ReadAll(io.LimitReader(file, ignoreLimit+1))
	file.Close()
	if err != nil {
		return nil, ioFault(err)
	}
	if len(data) > ignoreLimit {
		return nil, invalid(".gitignore exceeds 1 MiB")
	}
	if !utf8.Valid(data) {
		return nil, invalid(".gitignore is not UTF-8 text")
	}
	var domain []string
	if relative != "" {
		domain = strings.Split(relative, "/")
	}
	patterns := layer{}
	for _, line := range strings.Split(string(data), "\n") {
		line = strings.TrimSuffix(line, "\r")
		if !strings.HasSuffix(line, "\\ ") {
			line = strings.TrimRight(line, " ")
		}
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		patterns = append(patterns, gitignore.ParsePattern(line, domain))
	}
	return patterns, nil
}

// ignored asks the layers nearest first; the first that decides wins.
func ignored(layers []layer, relative string, directory bool) bool {
	path := strings.Split(relative, "/")
	for _, patterns := range layers {
		for index := len(patterns) - 1; index >= 0; index-- {
			if match := patterns[index].Match(path, directory); match != gitignore.NoMatch {
				return match == gitignore.Exclude
			}
		}
	}
	return false
}

// Watch tells which entries of one scanned directory matter to the index.
type Watch struct {
	Relative   string
	layers     []layer
	file       string
	ignoreFile bool
}

// Accepts reports whether a change of name in the directory needs a rescan.
func (watch Watch) Accepts(name string, directory bool) bool {
	if knowledge.Excluded(name) {
		return false
	}
	if name == ".gitignore" && watch.ignoreFile {
		return true
	}
	if watch.file != "" && watch.file != name {
		return false
	}
	return !ignored(watch.layers, join(watch.Relative, name), directory)
}

type frame struct {
	directory *os.Root
	entries   []string
	relative  string
	ignore    layer
}

type target struct {
	directory      *os.Root
	name, relative string
}

// Scan walks one source depth first.
type Scan struct {
	ctx        context.Context
	ocr        *OCR
	frames     []*frame
	inherited  []layer
	pending    *target
	omitted    *Entry
	base       string
	finished   bool
	incomplete bool
	watches    []Watch
}

func newFrame(ctx context.Context, directory *os.Root, relative string) (*frame, error) {
	ignore, err := loadIgnore(ctx, directory, relative)
	if err != nil {
		directory.Close()
		return nil, err
	}
	listing, err := directory.Open(".")
	if err != nil {
		directory.Close()
		return nil, ioFault(err)
	}
	names, err := listing.Readdirnames(-1)
	listing.Close()
	if err != nil {
		directory.Close()
		return nil, ioFault(err)
	}
	slices.Sort(names)
	return &frame{directory: directory, entries: names, relative: relative, ignore: ignore}, nil
}

func selectScan(ctx context.Context, directory *os.Root, parts []string, base string) (*Scan, error) {
	scan := &Scan{ctx: ctx, base: base}
	relative := ""
	for index, name := range parts {
		if err := cancelled(ctx); err != nil {
			directory.Close()
			return nil, err
		}
		ignore, err := loadIgnore(ctx, directory, relative)
		if err != nil {
			directory.Close()
			return nil, err
		}
		if ignore != nil {
			scan.inherited = append(scan.inherited, ignore)
		}
		scan.watches = append(scan.watches, Watch{Relative: relative, layers: scan.reversed(), file: name, ignoreFile: true})
		relative = join(relative, name)
		if knowledge.Excluded(name) {
			directory.Close()
			return nil, newFault(protocol.ErrorCodePermissionDenied, "protected source paths cannot be indexed")
		}
		metadata, err := directory.Lstat(name)
		if err != nil {
			directory.Close()
			return nil, ioFault(err)
		}
		if metadata.Mode()&os.ModeSymlink != 0 || !metadata.IsDir() && !metadata.Mode().IsRegular() {
			directory.Close()
			return nil, invalid("knowledge source must be a regular file or directory")
		}
		if ignored(scan.reversed(), relative, metadata.IsDir()) {
			directory.Close()
			scan.omitted = excluded(scan.locator(relative), "source is excluded by .gitignore")
			return scan, nil
		}
		if metadata.Mode().IsRegular() {
			if index+1 != len(parts) {
				directory.Close()
				return nil, invalid("knowledge source has a file as a parent")
			}
			scan.pending = &target{directory, name, relative}
			return scan, nil
		}
		if directory, err = descend(directory, name); err != nil {
			return nil, err
		}
	}
	root, err := newFrame(ctx, directory, relative)
	if err != nil {
		return nil, err
	}
	scan.frames = append(scan.frames, root)
	scan.watchFrame("")
	return scan, nil
}

// fileScan selects one explicit file; sibling ignore files do not apply.
func fileScan(ctx context.Context, directory *os.Root, name, base string) (*Scan, error) {
	metadata, err := directory.Lstat(name)
	if err != nil {
		directory.Close()
		return nil, ioFault(err)
	}
	if !metadata.Mode().IsRegular() {
		directory.Close()
		return nil, invalid("knowledge source must be a regular file")
	}
	return &Scan{ctx: ctx, base: base, pending: &target{directory, name, name}, watches: []Watch{{file: name}}}, nil
}

// SetOCR lets the scan read images and scanned PDF pages.
func (scan *Scan) SetOCR(ocr *OCR) { scan.ocr = ocr }

// Close releases the directories a scan still holds.
func (scan *Scan) Close() {
	for _, frame := range scan.frames {
		frame.directory.Close()
	}
	scan.frames = nil
	if scan.pending != nil {
		scan.pending.directory.Close()
		scan.pending = nil
	}
}

func (scan *Scan) reversed() []layer {
	layers := slices.Clone(scan.inherited)
	slices.Reverse(layers)
	return layers
}

// layers lists the retained ignore files nearest first.
func (scan *Scan) layers() []layer {
	var layers []layer
	for index := len(scan.frames) - 1; index >= 0; index-- {
		if scan.frames[index].ignore != nil {
			layers = append(layers, scan.frames[index].ignore)
		}
	}
	return append(layers, scan.reversed()...)
}

func (scan *Scan) locator(relative string) string {
	if scan.base == "" {
		return relative
	}
	return filepath.Join(scan.base, relative)
}

// watchFrame watches the innermost directory, or only the ignore file of a
// directory that could not be entered.
func (scan *Scan) watchFrame(unreadable string) {
	if unreadable != "" {
		scan.watches = append(scan.watches, Watch{Relative: unreadable, layers: scan.layers(), file: ".gitignore", ignoreFile: true})
		return
	}
	scan.watches = append(scan.watches, Watch{Relative: scan.frames[len(scan.frames)-1].relative, layers: scan.layers(), ignoreFile: true})
}

// TakeWatches returns the directories discovered since the last call.
func (scan *Scan) TakeWatches() []Watch {
	watches := scan.watches
	scan.watches = nil
	return watches
}

// Complete reports a scan that visited every directory.
func (scan *Scan) Complete() bool { return scan.finished && !scan.incomplete }

// Next returns the next entry, or nil at the end. reuse reports a file whose
// stamp the index still holds; watch receives directories as they are found,
// before their files are read.
func (scan *Scan) Next(reuse func(path string, stat Stat) (bool, error), watch func([]Watch)) (*Entry, error) {
	if scan.finished {
		return nil, nil
	}
	entry, err := scan.advance(reuse, watch)
	if entry == nil || err != nil {
		scan.finished = true
		scan.Close()
	}
	return entry, err
}

func (scan *Scan) extract(file *target, reuse func(string, Stat) (bool, error)) (*Entry, error) {
	defer file.directory.Close()
	path := scan.locator(file.relative)
	entry, err := read(scan.ctx, file, path, reuse, scan.ocr)
	if err != nil && code(err) == protocol.ErrorCodeInvalidRequest {
		var fault *protocol.Fault
		errors.As(err, &fault)
		return skipped(path, fault.Message), nil
	}
	return entry, err
}

func (scan *Scan) advance(reuse func(string, Stat) (bool, error), watch func([]Watch)) (*Entry, error) {
	for {
		if err := cancelled(scan.ctx); err != nil {
			return nil, err
		}
		watch(scan.TakeWatches())
		if entry := scan.omitted; entry != nil {
			scan.omitted = nil
			return entry, nil
		}
		if file := scan.pending; file != nil {
			scan.pending = nil
			return scan.extract(file, reuse)
		}
		if len(scan.frames) == 0 {
			return nil, nil
		}
		current := scan.frames[len(scan.frames)-1]
		if len(current.entries) == 0 {
			current.directory.Close()
			scan.frames = scan.frames[:len(scan.frames)-1]
			continue
		}
		name := current.entries[0]
		current.entries = current.entries[1:]
		if !utf8.ValidString(name) {
			return skipped(scan.locator(current.relative), "entry name is not UTF-8"), nil
		}
		relative := join(current.relative, name)
		path := scan.locator(relative)
		if knowledge.Excluded(name) {
			return excluded(path, "protected source path"), nil
		}
		if _, err := components(name, false); err != nil {
			return skipped(path, "unsupported source entry name"), nil
		}
		metadata, err := current.directory.Lstat(name)
		if err != nil {
			return nil, ioFault(err)
		}
		if metadata.Mode()&os.ModeSymlink != 0 || !metadata.IsDir() && !metadata.Mode().IsRegular() {
			return skipped(path, "source is not a regular file or directory"), nil
		}
		if ignored(scan.layers(), relative, metadata.IsDir()) {
			return excluded(path, "source is excluded by .gitignore"), nil
		}
		directory, err := current.directory.OpenRoot(".")
		if err != nil {
			return nil, ioFault(err)
		}
		if !metadata.IsDir() {
			return scan.extract(&target{directory, name, relative}, reuse)
		}
		if len(scan.frames) >= 64 {
			directory.Close()
			return nil, invalid("directory nesting exceeds 64 levels")
		}
		var child *frame
		if directory, err = descend(directory, name); err == nil {
			child, err = newFrame(scan.ctx, directory, relative)
		}
		if err != nil {
			if code(err) != protocol.ErrorCodeInvalidRequest {
				return nil, err
			}
			var fault *protocol.Fault
			errors.As(err, &fault)
			scan.incomplete = true
			scan.watchFrame(relative)
			watch(scan.TakeWatches())
			return skipped(path, fault.Message), nil
		}
		scan.frames = append(scan.frames, child)
		scan.watchFrame("")
	}
}
