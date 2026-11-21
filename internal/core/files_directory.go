// Directory paging follows harbor-file-service 727ce0a (Apache-2.0).
package core

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/sailry/sailry-harness/internal/protocol"
	"io"
	"lukechampine.com/blake3"
	"os"
	"slices"
	"strings"
)

func (files *Files) ListDirectory(ctx context.Context, relative string, after *protocol.DirectoryCursor) (protocol.Directory, error) {
	out := protocol.DefaultDirectory()
	out.Path = relative
	parts, err := fileComponents(relative, true, files.access, false)
	if err != nil {
		return out, err
	}
	if after != nil {
		decoded, e := hex.DecodeString(after.Revision)
		if e != nil || len(decoded) != 8 {
			return out, fileFault(protocol.ErrorCodeInvalidRequest, "invalid directory cursor")
		}
		if p, e := fileComponents(after.Name, false, ManualFiles, false); e != nil || len(p) != 1 || len(after.Revision) != 16 {
			return out, fileFault(protocol.ErrorCodeInvalidRequest, "invalid directory cursor")
		}
	}
	control := fileControl{context: ctx}
	if err := control.check(); err != nil {
		return out, err
	}
	dir, err := descendFiles(files.root, parts)
	if err != nil {
		return out, missingFileFault(err)
	}
	defer dir.Close()
	revision := func() (string, error) {
		f, e := dir.Open(".")
		if e != nil {
			return "", fileIOFault(e)
		}
		defer f.Close()
		id, e := fileNativeIdentity(f)
		if e != nil {
			return "", e
		}
		m, e := f.Stat()
		if e != nil {
			return "", fileIOFault(e)
		}
		raw, e := json.Marshal([]any{files.path, relative, id, m.ModTime(), m.Size()})
		if e != nil {
			return "", e
		}
		sum := blake3.Sum256(raw)
		return fmt.Sprintf("%x", sum[:8]), nil
	}
	out.Revision, err = revision()
	if err != nil {
		return out, err
	}
	if after != nil && after.Revision != out.Revision {
		return out, fileConflict()
	}
	handle, err := dir.Open(".")
	if err != nil {
		return out, fileIOFault(err)
	}
	defer handle.Close()
	compare := func(a, b protocol.FileEntry) int {
		ad, bd := a.Kind == protocol.EntryKindDirectory, b.Kind == protocol.EntryKindDirectory
		if ad != bd {
			if ad {
				return -1
			}
			return 1
		}
		return strings.Compare(a.Name, b.Name)
	}
	type candidate struct {
		value  protocol.FileEntry
		source os.DirEntry
	}
	var selected []candidate
	compareCandidate := func(a, b candidate) int { return compare(a.value, b.value) }
	for {
		if err := control.check(); err != nil {
			return out, err
		}
		entries, e := handle.ReadDir(128)
		for _, entry := range entries {
			name := entry.Name()
			if !validFileEntry(name) {
				out.UnsupportedNames++
				continue
			}
			// The listing orders by its own entry types; only a returned page is
			// inspected for sizes.
			kind := protocol.EntryKindOther
			switch mode := entry.Type(); {
			case mode&os.ModeSymlink != 0:
				kind = protocol.EntryKindSymlink
			case mode.IsDir():
				kind = protocol.EntryKindDirectory
			case mode.IsRegular():
				kind = protocol.EntryKindFile
			}
			item := protocol.FileEntry{Name: name, Kind: kind}
			if after != nil {
				cursor := protocol.FileEntry{Name: after.Name, Kind: protocol.EntryKindFile}
				if after.Directory {
					cursor.Kind = protocol.EntryKindDirectory
				}
				if compare(item, cursor) <= 0 {
					continue
				}
			}
			itemCandidate := candidate{value: item, source: entry}
			index, _ := slices.BinarySearchFunc(selected, itemCandidate, compareCandidate)
			selected = slices.Insert(selected, index, itemCandidate)
			if len(selected) > protocol.MAX_DIRECTORY_ENTRIES+1 {
				selected = selected[:protocol.MAX_DIRECTORY_ENTRIES+1]
			}
		}
		if e != nil {
			if !errors.Is(e, io.EOF) {
				return out, fileIOFault(e)
			}
			break
		}
	}
	current, e := revision()
	if e != nil {
		return out, e
	}
	if current != out.Revision {
		return out, fileConflict()
	}
	if len(selected) > protocol.MAX_DIRECTORY_ENTRIES {
		out.Truncated = true
		selected = selected[:protocol.MAX_DIRECTORY_ENTRIES]
		last := selected[len(selected)-1].value
		out.Next = &protocol.DirectoryCursor{Revision: out.Revision, Directory: last.Kind == protocol.EntryKindDirectory, Name: last.Name}
	}
	for _, item := range selected {
		if err := control.check(); err != nil {
			return out, err
		}
		m, e := fileDirectoryInfo(dir, item.source)
		if e != nil {
			return out, fileConflict()
		}
		value := item.value
		value.Size = uint64(m.Size())
		if modified := m.ModTime().UnixMilli(); modified > 0 {
			stamp := uint64(modified)
			value.ModifiedMs = &stamp
		}
		out.Entries = append(out.Entries, value)
	}
	return out, control.check()
}
