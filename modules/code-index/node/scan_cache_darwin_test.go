//go:build !sailry_no_code_index

package codeindex

import (
	"context"
	"os"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/core"
	"golang.org/x/sys/unix"
)

func TestAPFSWriteGenerationAndWritableMapping(t *testing.T) {
	t.Parallel()
	f := newFixture(t)
	var volume unix.Statfs_t
	if err := unix.Statfs(f.root, &volume); err != nil {
		t.Fatal(err)
	}
	if unix.ByteSliceToString(volume.Fstypename[:]) != "apfs" {
		t.Skip("requires an APFS volume")
	}
	const before = "pub fn before() {}\n"
	f.write("one.rs", before)
	reads := 0
	f.service.inspectText = func(files *core.Files, ctx context.Context, path string, limit int, stamp core.FileStamp, inspect func(*string) error) (core.FileStamp, error) {
		return files.InspectText(ctx, path, limit, stamp, func(source *string) error {
			reads++
			return inspect(source)
		})
	}
	baseline := f.search("absent")
	f.search("absent")
	if reads != 1 {
		t.Fatalf("APFS cold and warm scans performed %d body reads, want one", reads)
	}
	file, err := os.OpenFile(filepath.Join(f.root, "one.rs"), os.O_RDWR, 0)
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	data, err := unix.Mmap(int(file.Fd()), 0, len(before), unix.PROT_READ|unix.PROT_WRITE, unix.MAP_SHARED)
	if err != nil {
		t.Fatal(err)
	}
	defer unix.Munmap(data)
	for i, name := range []string{"second", "mapped"} {
		source := "pub fn " + name + "() {}\n"
		copy(data, source)
		reads = 0
		page := f.search(name)
		if reads != 2 || page.Partial || names2(page.Symbols) != name || page.Symbols[0].FileRevision != Hash(source) || page.IndexRevision != baseline.IndexRevision+uint64(i+1) {
			t.Fatalf("writable mmap must take the full-content path: reads=%d page=%+v", reads, page)
		}
		reads = 0
		f.search("absent")
		if reads != 1 {
			t.Fatalf("warm scan trusted an indeterminate write generation while mapped: %d reads", reads)
		}
	}
}
