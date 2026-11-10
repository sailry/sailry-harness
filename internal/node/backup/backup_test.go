package backup

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"testing"

	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// fixtureProfile writes the content one snapshot covers. The databases are
// opaque bytes: this package copies closed file families and never opens them.
func fixtureProfile(t *testing.T) (string, map[string][]byte) {
	t.Helper()
	profile := filepath.Join(t.TempDir(), "profile")
	large := make([]byte, partBytes+1024)
	if _, err := rand.Read(large); err != nil {
		t.Fatal(err)
	}
	files := map[string][]byte{
		"link.key":                      bytes.Repeat([]byte{7}, 32),
		"storage/node.sqlite":           []byte("node database"),
		"storage/node.sqlite-wal":       {},
		"storage/knowledge.sqlite":      []byte("knowledge database"),
		"attachments/one":               large,
		"plugins/packages/digest/run":   []byte("#!/bin/sh\n"),
		"plugins/packages/digest/data":  []byte("{}"),
		"artifacts/session/item/1.html": []byte("<p>artifact</p>"),
	}
	for name, body := range files {
		path := filepath.Join(profile, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
			t.Fatal(err)
		}
		mode := os.FileMode(0o600)
		if strings.HasSuffix(name, "/run") {
			mode = 0o700
		}
		if err := os.WriteFile(path, body, mode); err != nil {
			t.Fatal(err)
		}
	}
	// Content the storage owner does not list stays out of the archive.
	if err := os.WriteFile(filepath.Join(profile, "unlisted"), []byte("private"), 0o600); err != nil {
		t.Fatal(err)
	}
	return profile, files
}

func fixturePacked(t *testing.T, profile string) *Packed {
	t.Helper()
	snapshot, err := NewSnapshot(profile, protocol.NodeID{1, 2, 3})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = snapshot.Close() })
	for _, name := range databaseNames {
		if err := snapshot.Freeze(t.Context(), filepath.Join(profile, "storage", name)); err != nil {
			t.Fatal(err)
		}
	}
	roots := []string{"artifacts/session/item", "attachments/one", "link.key", "plugins/packages/digest"}
	if err := snapshot.Capture(t.Context(), roots); err != nil {
		t.Fatal(err)
	}
	packed, err := snapshot.Pack(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	return packed
}

// Over 1 s: sealing and opening run scrypt at the production work factor.
func TestBackupArchiveLayout(t *testing.T) {
	profile, files := fixtureProfile(t)
	packed := fixturePacked(t, profile)
	plain, err := os.ReadFile(packed.Path)
	if err != nil {
		t.Fatal(err)
	}

	t.Run("container", func(t *testing.T) {
		compressed, err := gzip.NewReader(bytes.NewReader(plain))
		if err != nil {
			t.Fatal(err)
		}
		archive := tar.NewReader(compressed)
		header, err := archive.Next()
		if err != nil || header.Name != manifestName || header.Typeflag != tar.TypeReg || header.Mode != 0o600 || header.Format != tar.FormatGNU {
			t.Fatalf("first entry = %#v %v", header, err)
		}
		body, err := io.ReadAll(archive)
		if err != nil {
			t.Fatal(err)
		}
		var fields map[string]json.RawMessage
		if err := json.Unmarshal(body, &fields); err != nil {
			t.Fatal(err)
		}
		keys := make([]string, 0, len(fields))
		for key := range fields {
			keys = append(keys, key)
		}
		slices.Sort(keys)
		if !slices.Equal(keys, []string{"files", "node", "original_profile", "version"}) {
			t.Fatalf("manifest fields = %v", keys)
		}
		var manifest Manifest
		if err := json.Unmarshal(body, &manifest); err != nil {
			t.Fatal(err)
		}
		if manifest.Version != 1 || manifest.OriginalProfile != profile || manifest.Node != (protocol.NodeID{1, 2, 3}) || len(manifest.Files) != len(files) {
			t.Fatalf("manifest = %#v", manifest)
		}
		var names []string
		for _, entry := range manifest.Files {
			header, err := archive.Next()
			if err != nil {
				t.Fatal(err)
			}
			content, err := io.ReadAll(archive)
			if err != nil {
				t.Fatal(err)
			}
			sum := sha256.Sum256(content)
			mode := uint32(0o600)
			if entry.Path == "plugins/packages/digest/run" && runtime.GOOS != "windows" {
				mode = 0o700
			}
			if header.Name != entry.Path || header.Typeflag != tar.TypeReg || uint64(header.Size) != entry.Size || uint32(header.Mode) != entry.Mode ||
				entry.Mode != mode || entry.SHA256 != hex.EncodeToString(sum[:]) || !bytes.Equal(content, files[entry.Path]) {
				t.Fatalf("entry %q differs from its manifest: %#v %#v", entry.Path, header, entry)
			}
			names = append(names, entry.Path)
		}
		if !slices.IsSorted(names) {
			t.Fatalf("entries are not in path order: %v", names)
		}
		if _, err := archive.Next(); !errors.Is(err, io.EOF) {
			t.Fatalf("trailing entry: %v", err)
		}
	})

	t.Run("refused archives", func(t *testing.T) {
		valid := func() Manifest {
			digest := strings.Repeat("0", 64)
			return Manifest{Version: 1, OriginalProfile: profile, Files: []File{
				{Path: "link.key", SHA256: digest, Mode: 0o600}, {Path: "storage/node.sqlite", SHA256: digest, Mode: 0o600},
			}}
		}
		for _, test := range []struct {
			name   string
			change func(*Manifest)
		}{
			{"version", func(manifest *Manifest) { manifest.Version = 2 }},
			{"relative profile", func(manifest *Manifest) { manifest.OriginalProfile = "profile" }},
			{"traversal", func(manifest *Manifest) { manifest.Files[0].Path = "../link.key" }},
			{"unlisted location", func(manifest *Manifest) {
				manifest.Files = append(manifest.Files, File{Path: "tmp/x", SHA256: manifest.Files[0].SHA256, Mode: 0o600})
			}},
			{"group mode", func(manifest *Manifest) { manifest.Files[0].Mode = 0o640 }},
			{"missing identity", func(manifest *Manifest) { manifest.Files = manifest.Files[1:] }},
			{"missing database", func(manifest *Manifest) { manifest.Files = manifest.Files[:1] }},
			{"log without database", func(manifest *Manifest) {
				manifest.Files = append(manifest.Files, File{Path: "storage/knowledge.sqlite-wal", SHA256: manifest.Files[0].SHA256, Mode: 0o600})
			}},
			{"duplicate", func(manifest *Manifest) { manifest.Files = append(manifest.Files, manifest.Files[0]) }},
			{"digest", func(manifest *Manifest) { manifest.Files[0].SHA256 = "abc" }},
		} {
			manifest := valid()
			if err := manifest.Validate(); err != nil {
				t.Fatal(err)
			}
			test.change(&manifest)
			if err := manifest.Validate(); code(err) != protocol.ErrorCodeInvalidRequest {
				t.Errorf("%s: %v", test.name, err)
			}
		}
		// The manifest must lead, and content must match it.
		for _, test := range []struct {
			name    string
			entries []tar.Header
		}{
			{"manifest not first", []tar.Header{{Name: "link.key", Typeflag: tar.TypeReg}}},
			{"symbolic manifest", []tar.Header{{Name: manifestName, Typeflag: tar.TypeSymlink, Linkname: "x"}}},
		} {
			var buffer bytes.Buffer
			compressed := gzip.NewWriter(&buffer)
			archive := tar.NewWriter(compressed)
			for _, header := range test.entries {
				if err := archive.WriteHeader(&header); err != nil {
					t.Fatal(err)
				}
			}
			if err := errors.Join(archive.Close(), compressed.Close()); err != nil {
				t.Fatal(err)
			}
			if _, err := Unpack(t.Context(), &buffer, t.TempDir()); code(err) != protocol.ErrorCodeInvalidRequest {
				t.Errorf("%s: %v", test.name, err)
			}
		}
		tampered := bytes.Clone(plain)
		tampered[len(tampered)/2] ^= 0xff
		if _, err := Unpack(t.Context(), bytes.NewReader(tampered), t.TempDir()); err == nil {
			t.Error("a corrupted archive was unpacked")
		}
	})

	t.Run("encryption and storage", func(t *testing.T) {
		passphrase := protocol.NewSecret("correct horse battery")
		directory := filepath.Join(t.TempDir(), "backups")
		plan := Plan{Local: &protocol.BackupLocal{Directory: directory, Enabled: true}, Auth: Auth{Passphrase: &passphrase}}
		id := uuid.NewString()
		record, err := Export(t.Context(), plan, packed, id)
		if err != nil {
			t.Fatal(err)
		}
		sealed, err := os.ReadFile(packed.Path)
		if err != nil {
			t.Fatal(err)
		}
		// age v1 with one scrypt stanza at work factor 18.
		header, _, _ := bytes.Cut(sealed, []byte("\n---"))
		lines := strings.Split(string(header), "\n")
		stanza := strings.Fields(lines[1])
		if lines[0] != "age-encryption.org/v1" || len(stanza) != 4 || stanza[0] != "->" || stanza[1] != "scrypt" || stanza[3] != "18" {
			t.Fatalf("age header = %q", header)
		}
		if bytes.Contains(sealed, []byte(manifestName)) || bytes.Contains(sealed, []byte(passphrase.Expose())) {
			t.Fatal("sealed archive exposes plaintext")
		}
		sum := sha256.Sum256(sealed)
		if record.ID != id || record.SizeBytes != uint64(len(sealed)) || record.Sha256 != hex.EncodeToString(sum[:]) || record.Files != uint32(len(files)) {
			t.Fatalf("record = %#v", record)
		}

		// `<id>.json` commits the `<id>/%06d.bin` segments of four mebibytes.
		entries, err := os.ReadDir(directory)
		if err != nil || len(entries) != 2 || entries[0].Name() != id || entries[1].Name() != id+".json" {
			t.Fatalf("layout = %v %v", entries, err)
		}
		body, err := os.ReadFile(filepath.Join(directory, id+".json"))
		if err != nil {
			t.Fatal(err)
		}
		var manifest remoteManifest
		if err := json.Unmarshal(body, &manifest); err != nil {
			t.Fatal(err)
		}
		if manifest.Version != 1 || manifest.Record != record || len(manifest.Parts) != 2 || manifest.Parts[0].Size != partBytes {
			t.Fatalf("commit marker = %#v", manifest)
		}
		var joined []byte
		for index, part := range manifest.Parts {
			segment, err := os.ReadFile(filepath.Join(directory, id, []string{"000000.bin", "000001.bin"}[index]))
			if err != nil {
				t.Fatal(err)
			}
			digest := sha256.Sum256(segment)
			if uint32(len(segment)) != part.Size || hex.EncodeToString(digest[:]) != part.SHA256 {
				t.Fatalf("segment %d differs from its metadata", index)
			}
			joined = append(joined, segment...)
		}
		if !bytes.Equal(joined, sealed) {
			t.Fatal("segments differ from the sealed archive")
		}

		page, err := List(t.Context(), plan)
		if err != nil || len(page.Records) != 1 || page.Records[0] != record || page.Partial || page.Locations[id] != (protocol.BackupLocations{Local: true}) {
			t.Fatalf("list = %#v %v", page, err)
		}

		fetched, file, err := Fetch(t.Context(), plan, id, protocol.BackupLocationLocal, t.TempDir())
		if err != nil || fetched != record {
			t.Fatalf("fetch = %#v %v", fetched, err)
		}
		defer file.Close()
		wrong := protocol.NewSecret("incorrect horse battery")
		if _, err := Open(t.Context(), file, t.TempDir(), wrong); code(err) != protocol.ErrorCodeInvalidRequest || strings.Contains(err.Error(), wrong.Expose()) {
			t.Fatalf("wrong passphrase: %v", err)
		}
		if _, err := file.Seek(0, io.SeekStart); err != nil {
			t.Fatal(err)
		}
		opened, err := Open(t.Context(), file, t.TempDir(), passphrase)
		if err != nil {
			t.Fatal(err)
		}
		defer opened.Close()
		restored, err := io.ReadAll(opened)
		if err != nil || !bytes.Equal(restored, plain) {
			t.Fatalf("decrypted archive differs: %v", err)
		}

		// A changed segment is refused before decryption.
		segment := filepath.Join(directory, id, "000001.bin")
		if err := os.WriteFile(segment, []byte("changed"), 0o600); err != nil {
			t.Fatal(err)
		}
		if _, _, err := Fetch(t.Context(), plan, id, protocol.BackupLocationLocal, t.TempDir()); code(err) != protocol.ErrorCodeInvalidRequest {
			t.Fatalf("changed segment: %v", err)
		}
		cancelled, cancel := context.WithCancel(t.Context())
		cancel()
		if _, err := List(cancelled, plan); code(err) != protocol.ErrorCodeCancelled {
			t.Fatalf("cancelled list: %v", err)
		}
	})
}
