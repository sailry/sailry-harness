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
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"testing"

	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/net/webdav"
)

const fixturePassphrase = "correct horse battery"

// fixtureProfile writes the content one snapshot covers. The databases are
// opaque bytes: the storage owner writes the copies and this package never
// opens them.
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
		"storage/knowledge.sqlite":      []byte("knowledge database"),
		"attachments/one":               large,
		"plugins/packages/digest/run":   []byte("#!/bin/sh\n"),
		"plugins/packages/digest/data":  []byte("{}"),
		"artifacts/session/item/1.html": []byte("<p>artifact</p>"),
	}
	for name, body := range files {
		if strings.HasPrefix(name, "storage/") {
			continue
		}
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

// fixtureSnapshot captures the listed roots after both database copies.
func fixtureSnapshot(t *testing.T, profile string, files map[string][]byte, roots []string) *Snapshot {
	t.Helper()
	return fixtureSnapshotCopied(t, profile, files, roots, nil)
}

// fixtureSnapshotCopied is fixtureSnapshot with a function the snapshot calls
// after it copied a file and before it verifies the copy.
func fixtureSnapshotCopied(t *testing.T, profile string, files map[string][]byte, roots []string, copied func(relative string)) *Snapshot {
	t.Helper()
	snapshot, err := NewSnapshot(profile, protocol.NodeID{1, 2, 3})
	if err != nil {
		t.Fatal(err)
	}
	snapshot.copied = copied
	t.Cleanup(func() { _ = snapshot.Close() })
	for _, name := range databaseNames {
		write := func(_ context.Context, target string) error {
			return os.WriteFile(target, files["storage/"+name], 0o600)
		}
		if err := snapshot.Freeze(t.Context(), name, write); err != nil {
			t.Fatal(err)
		}
	}
	if err := snapshot.Capture(t.Context(), roots); err != nil {
		t.Fatal(err)
	}
	return snapshot
}

func fixturePack(t *testing.T, snapshot *Snapshot) *Packed {
	t.Helper()
	packed, err := snapshot.Pack(t.Context(), protocol.NewSecret(fixturePassphrase))
	if err != nil {
		t.Fatal(err)
	}
	return packed
}

var fixtureRoots = []string{"artifacts/session/item", "attachments/one", "link.key", "plugins/packages/digest"}

// Over 1 s: sealing and opening run scrypt at the production work factor.
func TestBackupArchiveLayout(t *testing.T) {
	profile, files := fixtureProfile(t)
	snapshot := fixtureSnapshot(t, profile, files, fixtureRoots)
	var buffer bytes.Buffer
	if err := snapshot.archive(t.Context(), &buffer); err != nil {
		t.Fatal(err)
	}
	plain := buffer.Bytes()
	packed := fixturePack(t, snapshot)

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
			{"link in place of the identity", func(manifest *Manifest) {
				manifest.Files = manifest.Files[1:]
				manifest.Links = []Link{{Path: "link.key", Target: "/etc/passwd"}}
			}},
			{"link in storage", func(manifest *Manifest) { manifest.Links = []Link{{Path: "storage/backups", Target: "/tmp"}} }},
			{"link with a file of the same name", func(manifest *Manifest) {
				manifest.Files = append(manifest.Files, File{Path: "attachments/a", SHA256: manifest.Files[0].SHA256, Mode: 0o600})
				manifest.Links = []Link{{Path: "attachments/a", Target: "b"}}
			}},
			{"file written through a link", func(manifest *Manifest) {
				manifest.Files = append(manifest.Files, File{Path: "attachments/a/b", SHA256: manifest.Files[0].SHA256, Mode: 0o600})
				manifest.Links = []Link{{Path: "attachments/a", Target: "/tmp"}}
			}},
			{"link created through a link", func(manifest *Manifest) {
				manifest.Links = []Link{{Path: "attachments/a", Target: "/tmp"}, {Path: "attachments/a/b", Target: "c"}}
			}},
			{"empty link target", func(manifest *Manifest) { manifest.Links = []Link{{Path: "attachments/a", Target: ""}} }},
			{"skip count below the list", func(manifest *Manifest) {
				manifest.Skipped = []Skipped{{Path: "attachments/a", Reason: skipChanged}}
			}},
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

	t.Run("local target", func(t *testing.T) {
		// Only a folder inside the profile, or one that contains it, is
		// refused; neighbours are not, however they are spelled.
		root := t.TempDir()
		resolved, err := filepath.EvalSymlinks(root)
		if err != nil {
			t.Fatal(err)
		}
		profile := filepath.Join(root, "x", "profile")
		for _, path := range []string{profile, filepath.Join(root, "x", "profile-backups"), filepath.Join(root, "x", "backups"), filepath.Join(root, "other")} {
			if err := os.MkdirAll(path, 0o700); err != nil {
				t.Fatal(err)
			}
		}
		// The temporary root reached through a symbolic link, as /var is on macOS.
		linked := filepath.Join(t.TempDir(), "link")
		if err := os.Symlink(root, linked); err != nil {
			t.Skip("symbolic links are unavailable")
		}
		for _, test := range []struct {
			name    string
			target  string
			profile string
			refused bool
		}{
			{"the profile", profile, profile, true},
			{"inside", filepath.Join(profile, "backups"), profile, true},
			{"missing folder inside", filepath.Join(profile, "a", "b"), profile, true},
			{"parent", filepath.Join(root, "x"), profile, true},
			{"ancestor", root, profile, true},
			{"inside through a link", filepath.Join(linked, "x", "profile", "backups"), profile, true},
			{"inside by the resolved spelling", filepath.Join(resolved, "x", "profile", "backups"), profile, true},
			{"parent through a link", filepath.Join(linked, "x"), profile, true},
			{"sibling", filepath.Join(root, "x", "backups"), profile, false},
			{"missing sibling", filepath.Join(root, "x", "new", "backups"), profile, false},
			{"prefix-similar name", filepath.Join(root, "x", "profile-backups"), profile, false},
			{"profile with the longer name", profile, filepath.Join(root, "x", "profile-backups"), false},
			{"shared grandparent", filepath.Join(root, "other"), profile, false},
			{"sibling through a link", filepath.Join(linked, "x", "backups"), profile, false},
			{"sibling by the resolved spelling", filepath.Join(resolved, "x", "backups"), profile, false},
			{"sibling of a linked profile", filepath.Join(resolved, "other"), filepath.Join(linked, "x", "profile"), false},
			{"separate temporary directory", t.TempDir(), profile, false},
		} {
			err := LocalTarget(test.target, test.profile)
			if code := protocol.ErrorCode(""); err != nil {
				fault, _ := err.(*protocol.Fault)
				if fault != nil {
					code = fault.Code
				}
				if !test.refused || code != protocol.ErrorCodePermissionDenied {
					t.Errorf("%s: %v", test.name, err)
				}
			} else if test.refused {
				t.Errorf("%s: accepted", test.name)
			}
		}
	})

	t.Run("encryption and storage", func(t *testing.T) {
		passphrase := protocol.NewSecret("correct horse battery")
		directory := filepath.Join(t.TempDir(), "backups")
		plan := Plan{Local: &protocol.BackupLocal{Directory: directory, Enabled: true}, Auth: Auth{Passphrase: &passphrase}}
		id := uuid.NewString()
		// The archive is sealed as it is written: the snapshot holds no plain archive.
		if leftovers, err := filepath.Glob(filepath.Join(snapshot.Directory, "archive-*")); err != nil || len(leftovers) != 0 {
			t.Fatalf("plain archive on disk: %v %v", leftovers, err)
		}
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
		// A wrong passphrase, an input that is no archive and a damaged
		// archive are told apart.
		wrong := protocol.NewSecret("incorrect horse battery")
		if _, err := Open(t.Context(), file, wrong); code(err) != protocol.ErrorCodeInvalidRequest ||
			!strings.Contains(err.Error(), "passphrase is incorrect") || strings.Contains(err.Error(), wrong.Expose()) {
			t.Fatalf("wrong passphrase: %v", err)
		}
		if _, err := Open(t.Context(), strings.NewReader("not an archive"), passphrase); code(err) != protocol.ErrorCodeInvalidRequest ||
			!strings.Contains(err.Error(), "not a Sailry backup") {
			t.Fatalf("foreign file: %v", err)
		}
		opened, err := Open(t.Context(), bytes.NewReader(sealed[:len(sealed)-1024]), passphrase)
		if err != nil {
			t.Fatal(err)
		}
		if _, err := Stage(t.Context(), opened, t.TempDir(), &record, "", 0, ""); code(err) != protocol.ErrorCodeInvalidRequest ||
			!strings.Contains(err.Error(), "damaged or incomplete") {
			t.Fatalf("truncated archive: %v", err)
		}
		// A destination that cannot be written is not a passphrase problem.
		if _, err := file.Seek(0, io.SeekStart); err != nil {
			t.Fatal(err)
		}
		if opened, err = Open(t.Context(), file, passphrase); err != nil {
			t.Fatal(err)
		}
		if _, err := Stage(t.Context(), opened, filepath.Join(t.TempDir(), "missing"), &record, "", 0, ""); code(err) != protocol.ErrorCodeUnavailable {
			t.Fatalf("unwritable staging: %v", err)
		}
		if _, err := file.Seek(0, io.SeekStart); err != nil {
			t.Fatal(err)
		}
		if opened, err = Open(t.Context(), file, passphrase); err != nil {
			t.Fatal(err)
		}
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

	t.Run("a failed online copy retains the committed local backup", func(t *testing.T) {
		for _, test := range []struct {
			name   string
			status int
			code   protocol.ErrorCode
		}{
			{"refused", http.StatusForbidden, protocol.ErrorCodePermissionDenied},
			{"write outcome unknown", http.StatusServiceUnavailable, protocol.ErrorCodeOutcomeUnknown},
		} {
			t.Run(test.name, func(t *testing.T) {
				handler := &webdav.Handler{FileSystem: webdav.Dir(t.TempDir()), LockSystem: webdav.NewMemLS()}
				server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
					if test.status == http.StatusForbidden || request.Method == http.MethodPut {
						writer.WriteHeader(test.status)
						return
					}
					handler.ServeHTTP(writer, request)
				}))
				t.Cleanup(server.Close)
				plan := Plan{
					Local:         &protocol.BackupLocal{Directory: t.TempDir(), Enabled: true},
					Destination:   &protocol.BackupDestination{Kind: protocol.BackupKindWebdav, Endpoint: server.URL},
					OnlineEnabled: true, Transport: server.Client().Transport,
				}
				id := uuid.NewString()
				record, err := Export(t.Context(), plan, packed, id)
				var failure *protocol.Fault
				if !errors.As(err, &failure) || failure.Code != test.code || record.ID != id || failure.Details == nil || failure.Details.Kind != "backup_partial" {
					t.Fatalf("partial export: %+v, %v", record, err)
				}
				// Durable recovery must retain the completed record and the remote
				// failure's certainty after the fault crosses the wire or storage.
				body, err := json.Marshal(failure)
				var decoded protocol.Fault
				if err != nil || json.Unmarshal(body, &decoded) != nil || decoded.Details == nil {
					t.Fatalf("partial fault round trip: %s, %v", body, err)
				}
				detail, ok := decoded.Details.Data.(*protocol.FaultDetailsBackupPartialPayload)
				if !ok || detail.Record != record || detail.Location != protocol.BackupLocationOnline || detail.Failure.Code != test.code {
					t.Fatalf("partial fault lost committed evidence: %+v", decoded)
				}
				local := Plan{Local: plan.Local}
				page, err := List(t.Context(), local)
				if err != nil || len(page.Records) != 1 || page.Records[0] != record || page.Locations[id] != (protocol.BackupLocations{Local: true}) {
					t.Fatalf("local copy disappeared: %+v, %v", page, err)
				}
				_, sealed, err := Fetch(t.Context(), local, id, protocol.BackupLocationLocal, t.TempDir())
				if err != nil {
					t.Fatalf("local copy is not restorable: %v", err)
				}
				_ = sealed.Close()
			})
		}
	})

	t.Run("retention and damaged records", func(t *testing.T) {
		passphrase := protocol.NewSecret(fixturePassphrase)
		directory := filepath.Join(t.TempDir(), "backups")
		plan := Plan{Local: &protocol.BackupLocal{Directory: directory, Enabled: true}, Auth: Auth{Passphrase: &passphrase}}
		node := snapshot.Manifest.Node
		// publish stores one backup as made by owner at the given time.
		publish := func(owner protocol.NodeID, created uint64) string {
			t.Helper()
			id := uuid.NewString()
			if _, err := Export(t.Context(), plan, packed, id); err != nil {
				t.Fatal(err)
			}
			path := filepath.Join(directory, id+".json")
			body, err := os.ReadFile(path)
			if err != nil {
				t.Fatal(err)
			}
			var manifest remoteManifest
			if err := json.Unmarshal(body, &manifest); err != nil {
				t.Fatal(err)
			}
			manifest.Record.Node, manifest.Record.CreatedMs = owner, created
			if body, err = json.Marshal(manifest); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(path, body, 0o600); err != nil {
				t.Fatal(err)
			}
			return id
		}
		oldest, older, newer, newest := publish(node, 1000), publish(node, 2000), publish(node, 3000), publish(node, 4000)
		foreign := publish(protocol.NodeID{9}, 500)
		damaged, truncated := uuid.NewString(), uuid.NewString()
		if err := os.WriteFile(filepath.Join(directory, damaged+".json"), []byte("not a record"), 0o600); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(directory, truncated+".json"), []byte(`{"version":1,"record":{"id":"`), 0o600); err != nil {
			t.Fatal(err)
		}
		listed := func(plan Plan) (ids []string, partial bool) {
			t.Helper()
			page, err := List(t.Context(), plan)
			if err != nil {
				t.Fatalf("list: %v", err)
			}
			for _, record := range page.Records {
				ids = append(ids, record.ID)
			}
			return ids, page.Partial
		}
		// Damaged records are passed over and flagged; the rest is offered.
		if ids, partial := listed(plan); !slices.Equal(ids, []string{newest, newer, older, oldest, foreign}) || !partial {
			t.Fatalf("list beside damaged records = %v, partial %v", ids, partial)
		}
		// A second location that does not answer does not hide the first.
		offline := plan
		offline.Destination = &protocol.BackupDestination{Kind: protocol.BackupKindWebdav, Endpoint: "https://backup.invalid/dav"}
		if ids, partial := listed(offline); len(ids) != 5 || !partial {
			t.Fatalf("list with an unreachable location = %v, partial %v", ids, partial)
		}
		offline.Local = nil
		if _, err := List(t.Context(), offline); err == nil {
			t.Fatal("a list no location answered succeeded")
		}

		// Keeping all is the default: nothing is ever deleted unasked.
		if removed, err := Prune(t.Context(), plan, node); err != nil || removed != 0 {
			t.Fatalf("prune without a limit = %d, %v", removed, err)
		}
		plan.Keep = 2
		if removed, err := Prune(t.Context(), plan, node); err != nil || removed != 2 {
			t.Fatalf("prune = %d, %v", removed, err)
		}
		// The Node's two newest stay, with another Node's backup and the
		// records that could not be read.
		if ids, _ := listed(plan); !slices.Equal(ids, []string{newest, newer, foreign}) {
			t.Fatalf("after retention = %v", ids)
		}
		for _, id := range []string{oldest, older} {
			for _, name := range []string{id, id + ".json"} {
				if _, err := os.Lstat(filepath.Join(directory, name)); !os.IsNotExist(err) {
					t.Fatalf("pruned backup left %s: %v", name, err)
				}
			}
		}
		for _, name := range []string{damaged + ".json", truncated + ".json", foreign + ".json", filepath.Join(foreign, "000000.bin")} {
			if _, err := os.Lstat(filepath.Join(directory, name)); err != nil {
				t.Fatalf("retention touched %s: %v", name, err)
			}
		}
		if removed, err := Prune(t.Context(), plan, node); err != nil || removed != 0 {
			t.Fatalf("second prune = %d, %v", removed, err)
		}
		// A kept backup still restores.
		if _, file, err := Fetch(t.Context(), plan, newer, protocol.BackupLocationLocal, t.TempDir()); err != nil {
			t.Fatalf("fetch after retention: %v", err)
		} else {
			_ = file.Close()
		}
	})

	t.Run("a refused export leaves nothing behind", func(t *testing.T) {
		directory := filepath.Join(t.TempDir(), "backups")
		plan := Plan{Local: &protocol.BackupLocal{Directory: directory, Enabled: true}}
		// An archive without content has no segment to commit.
		empty := *packed
		empty.Path, empty.Size = filepath.Join(t.TempDir(), "empty"), 0
		if err := os.WriteFile(empty.Path, nil, 0o600); err != nil {
			t.Fatal(err)
		}
		if _, err := Export(t.Context(), plan, &empty, uuid.NewString()); code(err) != protocol.ErrorCodeInvalidRequest {
			t.Fatalf("empty archive: %v", err)
		}
		if entries, err := os.ReadDir(directory); err != nil || len(entries) != 0 {
			t.Fatalf("refused export left %v: %v", entries, err)
		}
	})
}
