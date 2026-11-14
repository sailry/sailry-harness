//go:build windows && integration

package core

import (
	"context"
	"errors"
	"path/filepath"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/sys/windows"
)

func TestWindowsDirectoryWithLockedFile(t *testing.T) {
	files, directory := testFiles(t)
	const name = "locked.dat"
	const content = "locked fixture"
	putFileFixture(t, directory, name, content)
	putFileFixture(t, directory, "readable.txt", "readable fixture")
	path, err := windows.UTF16PtrFromString(filepath.Join(directory, name))
	if err != nil {
		t.Fatal(err)
	}
	handle, err := windows.CreateFile(path, windows.GENERIC_READ|windows.GENERIC_WRITE, 0, nil, windows.OPEN_EXISTING, windows.FILE_ATTRIBUTE_NORMAL, 0)
	if err != nil {
		t.Fatal(err)
	}
	defer windows.CloseHandle(handle)
	if _, err := files.root.Lstat(name); !errors.Is(err, windows.ERROR_SHARING_VIOLATION) {
		t.Fatalf("fixture does not reproduce a share-locked entry: %v", err)
	}
	listing, err := files.ListDirectory(context.Background(), "", nil)
	if err != nil {
		t.Fatalf("a locked child prevented directory enumeration: %v", err)
	}
	index := slices.IndexFunc(listing.Entries, func(entry protocol.FileEntry) bool { return entry.Name == name })
	if len(listing.Entries) != 2 || index < 0 {
		t.Fatalf("directory omitted a visible entry: %+v", listing.Entries)
	}
	entry := listing.Entries[index]
	if entry.Kind != protocol.EntryKindFile || entry.Size != uint64(len(content)) || entry.ModifiedMs == nil {
		t.Fatalf("enumerated metadata lost: %+v", entry)
	}
	if file, err := files.Open(name); err == nil {
		file.Close()
		t.Fatal("listing a locked file granted read access")
	}
	if _, err := files.ReadContent(context.Background(), "readable.txt"); err != nil {
		t.Fatal(err)
	}
}
