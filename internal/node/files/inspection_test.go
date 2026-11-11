package files

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"image"
	"image/color"
	"image/gif"
	"image/png"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestFileInspectionPages(t *testing.T) {
	t.Parallel()
	root, fs := filesFixture(t)
	ctx := context.Background()
	if err := os.Mkdir(filepath.Join(root, "directory"), 0o700); err != nil {
		t.Fatal(err)
	}
	for index := 0; index < 501; index++ {
		if err := os.WriteFile(filepath.Join(root, fmt.Sprintf("file-%03d.txt", index)), nil, 0o600); err != nil {
			t.Fatal(err)
		}
	}
	page, err := fs.ListDirectory(ctx, "", nil)
	if err != nil || len(page.Entries) != 500 || page.Entries[0].Name != "directory" || page.Next == nil {
		t.Fatalf("first directory page: %+v %v", page, err)
	}
	last, err := fs.ListDirectory(ctx, "", page.Next)
	if err != nil || len(last.Entries) != 2 || last.Next != nil {
		t.Fatalf("last directory page: %+v %v", last, err)
	}
	if err = os.WriteFile(filepath.Join(root, "new.txt"), nil, 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err = fs.ListDirectory(ctx, "", page.Next); err == nil {
		t.Fatal("stale directory cursor was accepted")
	}
	text := "zero\nneedle\nmiddle\nneedle\nlast\n"
	if err = os.WriteFile(filepath.Join(root, "main.go"), []byte(text), 0o600); err != nil {
		t.Fatal(err)
	}
	fileType := "go"
	options := protocol.FileSearch{Query: "needle", Mode: protocol.SearchModeContent, Context: 1, Limit: 3, FileType: &fileType}
	result, err := Search(ctx, fs, options)
	if err != nil || len(result.Matches) != 3 || result.NextOffset == nil || *result.NextOffset != 3 || result.Matches[0].Matched || !result.Matches[1].Matched {
		t.Fatalf("search page: %+v %v", result, err)
	}
	options.Offset = 3
	result, err = Search(ctx, fs, options)
	if err != nil || len(result.Matches) != 2 || result.NextOffset != nil || result.Matches[0].LineNumber != 4 {
		t.Fatalf("search continuation: %+v %v", result, err)
	}
	options.Offset = 0
	options.Mode = protocol.SearchModeCount
	result, err = Search(ctx, fs, options)
	if err != nil || len(result.Counts) != 1 || result.Counts[0].Count != 2 {
		t.Fatalf("search count: %+v %v", result, err)
	}

	// A browsed entry carries its modified time, optional on the wire.
	t.Run("modified time", func(t *testing.T) {
		directory := t.TempDir()
		stamp := time.Date(2026, 11, 8, 14, 32, 10, 0, time.UTC)
		if err := os.Mkdir(filepath.Join(directory, "folder"), 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(directory, "notes.md"), []byte("notes"), 0o600); err != nil {
			t.Fatal(err)
		}
		for _, name := range []string{"folder", "notes.md"} {
			if err := os.Chtimes(filepath.Join(directory, name), stamp, stamp); err != nil {
				t.Fatal(err)
			}
		}
		listing, err := Browse(ctx, &directory, nil)
		if err != nil || len(listing.Directory.Entries) != 2 {
			t.Fatalf("browsing %s: %+v %v", directory, listing, err)
		}
		for _, entry := range listing.Directory.Entries {
			if entry.ModifiedMs == nil || *entry.ModifiedMs != uint64(stamp.UnixMilli()) {
				t.Errorf("%s was modified at %v, want %d", entry.Name, entry.ModifiedMs, stamp.UnixMilli())
			}
		}
		encoded, err := json.Marshal(listing.Directory.Entries[1])
		if want := fmt.Sprintf(`{"modified_ms":%d,"name":"notes.md","kind":"file","size":5}`, stamp.UnixMilli()); err != nil || string(encoded) != want {
			t.Errorf("the entry encodes as %s %v, want %s", encoded, err, want)
		}
		var decoded protocol.FileEntry
		if err := json.Unmarshal(encoded, &decoded); err != nil || decoded.ModifiedMs == nil || *decoded.ModifiedMs != uint64(stamp.UnixMilli()) {
			t.Errorf("the entry decodes as %+v %v", decoded, err)
		}
		if err := json.Unmarshal([]byte(`{"name":"a","kind":"file","size":1}`), &decoded); err != nil || decoded.ModifiedMs != nil {
			t.Errorf("an entry without a time decodes as %+v %v", decoded, err)
		}
	})

	// A directory that is not there is told from one the Node may not read,
	// and both from a Node that cannot answer.
	t.Run("browse faults", func(t *testing.T) {
		directory := t.TempDir()
		missing := filepath.Join(directory, "gone")
		if _, err := Browse(ctx, &missing, nil); fault(err).Code != protocol.ErrorCodeNotFound {
			t.Errorf("a missing directory answers %v", err)
		}
		locked := filepath.Join(directory, "locked")
		if err := os.MkdirAll(filepath.Join(locked, "inner"), 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.Chmod(locked, 0); err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = os.Chmod(locked, 0o700) })
		// A privileged process, and a file system without modes, read it
		// all the same.
		if _, err := os.ReadDir(locked); err == nil {
			return
		}
		inner := filepath.Join(locked, "inner")
		for _, path := range []string{locked, inner} {
			if _, err := Browse(ctx, &path, nil); fault(err).Code != protocol.ErrorCodePermissionDenied {
				t.Errorf("%s answers %v", path, err)
			}
		}
	})
}
func TestImagePreparation(t *testing.T) {
	t.Parallel()
	source := image.NewNRGBA(image.Rect(0, 0, 3, 2))
	source.Set(0, 0, color.NRGBA{R: 255, A: 255})
	var encoded bytes.Buffer
	if err := png.Encode(&encoded, source); err != nil {
		t.Fatal(err)
	}
	facts, sent, data, err := PrepareImage(encoded.Bytes())
	if err != nil || facts.Width != 3 || sent != nil || !bytes.Equal(data, encoded.Bytes()) {
		t.Fatalf("small image: %+v %+v %v", facts, sent, err)
	}
	palette := color.Palette{color.Black, color.White}
	frame := image.NewPaletted(image.Rect(0, 0, 3, 2), palette)
	encoded.Reset()
	if err = gif.EncodeAll(&encoded, &gif.GIF{Image: []*image.Paletted{frame, frame}, Delay: []int{1, 1}}); err != nil {
		t.Fatal(err)
	}
	_, sent, data, err = PrepareImage(encoded.Bytes())
	if err != nil || sent == nil || sent.MimeType != "image/png" || bytes.HasPrefix(data, []byte("GIF")) {
		t.Fatalf("animated image: %+v %v", sent, err)
	}
	rotated := orientImage(source, 6)
	if rotated.Bounds().Dx() != 2 || rotated.Bounds().Dy() != 3 || rotated.At(1, 0) != source.At(0, 0) {
		t.Fatalf("orientation was not applied: %v", rotated.Bounds())
	}
	if _, _, _, err = PrepareImage([]byte("not an image")); err == nil {
		t.Fatal("invalid image was accepted")
	}
}
