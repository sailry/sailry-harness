//go:build integration

package backup

import (
	"bufio"
	"bytes"
	"encoding/xml"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"slices"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/net/webdav"
)

const (
	fixtureIdentity = "backup-user"
	fixtureSecret   = "backup-secret-value"
)

// fixtureWebDAV is the golang.org/x/net/webdav handler over a temporary
// directory behind HTTP Basic authentication.
func fixtureWebDAV(t *testing.T) (*httptest.Server, string) {
	t.Helper()
	directory := t.TempDir()
	handler := &webdav.Handler{FileSystem: webdav.Dir(directory), LockSystem: webdav.NewMemLS()}
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		identity, secret, ok := request.BasicAuth()
		if !ok || identity != fixtureIdentity || secret != fixtureSecret {
			writer.Header().Set("WWW-Authenticate", `Basic realm="backup"`)
			http.Error(writer, "unauthorized", http.StatusUnauthorized)
			return
		}
		handler.ServeHTTP(writer, request)
	}))
	t.Cleanup(server.Close)
	return server, directory
}

// fixtureS3 is a minimal in-process S3-compatible handler, not a real object
// store: path-style PUT, GET, HEAD and DELETE of objects and ListObjectsV2 with
// a delimiter. It checks that requests carry a SigV4 authorization naming the
// access key, but does not verify signatures.
type fixtureS3 struct {
	mu      sync.Mutex
	bucket  string
	objects map[string][]byte
}

func (store *fixtureS3) ServeHTTP(writer http.ResponseWriter, request *http.Request) {
	if !strings.HasPrefix(request.Header.Get("Authorization"), "AWS4-HMAC-SHA256 Credential="+fixtureIdentity+"/") {
		http.Error(writer, "<Error><Code>AccessDenied</Code></Error>", http.StatusForbidden)
		return
	}
	bucket, key, _ := strings.Cut(strings.TrimPrefix(request.URL.Path, "/"), "/")
	if bucket != store.bucket {
		http.Error(writer, "<Error><Code>NoSuchBucket</Code></Error>", http.StatusNotFound)
		return
	}
	store.mu.Lock()
	defer store.mu.Unlock()
	switch {
	case request.Method == http.MethodGet && key == "":
		store.list(writer, request)
	case request.Method == http.MethodPut:
		body, err := fixtureS3Body(request)
		if err != nil {
			http.Error(writer, "<Error><Code>IncompleteBody</Code></Error>", http.StatusBadRequest)
			return
		}
		store.objects[key] = body
		writer.Header().Set("ETag", `"fixture"`)
	case request.Method == http.MethodDelete:
		delete(store.objects, key)
		writer.WriteHeader(http.StatusNoContent)
	case request.Method == http.MethodGet || request.Method == http.MethodHead:
		body, ok := store.objects[key]
		if !ok {
			http.Error(writer, "<Error><Code>NoSuchKey</Code></Error>", http.StatusNotFound)
			return
		}
		writer.Header().Set("ETag", `"fixture"`)
		http.ServeContent(writer, request, "", time.Unix(1_700_000_000, 0), bytes.NewReader(body))
	default:
		http.Error(writer, "<Error><Code>MethodNotAllowed</Code></Error>", http.StatusMethodNotAllowed)
	}
}

func (store *fixtureS3) list(writer http.ResponseWriter, request *http.Request) {
	type content struct {
		Key          string
		Size         int
		LastModified string
		ETag         string
	}
	type prefix struct{ Prefix string }
	result := struct {
		XMLName        xml.Name `xml:"ListBucketResult"`
		Name           string
		Prefix         string
		Delimiter      string
		IsTruncated    bool
		Contents       []content
		CommonPrefixes []prefix
	}{Name: store.bucket, Prefix: request.URL.Query().Get("prefix"), Delimiter: request.URL.Query().Get("delimiter")}
	keys := make([]string, 0, len(store.objects))
	for key := range store.objects {
		keys = append(keys, key)
	}
	slices.Sort(keys)
	for _, key := range keys {
		rest, ok := strings.CutPrefix(key, result.Prefix)
		if !ok {
			continue
		}
		if index := strings.Index(rest, result.Delimiter); result.Delimiter != "" && index >= 0 {
			common := prefix{Prefix: result.Prefix + rest[:index+1]}
			if !slices.Contains(result.CommonPrefixes, common) {
				result.CommonPrefixes = append(result.CommonPrefixes, common)
			}
			continue
		}
		result.Contents = append(result.Contents, content{Key: key, Size: len(store.objects[key]), LastModified: "2023-11-14T22:13:20.000Z", ETag: `"fixture"`})
	}
	writer.Header().Set("Content-Type", "application/xml")
	_ = xml.NewEncoder(writer).Encode(result)
}

// fixtureS3Body reads a plain or aws-chunked request body.
func fixtureS3Body(request *http.Request) ([]byte, error) {
	if !strings.HasPrefix(request.Header.Get("X-Amz-Content-Sha256"), "STREAMING-") {
		return io.ReadAll(request.Body)
	}
	reader := bufio.NewReader(request.Body)
	var body []byte
	for {
		line, err := reader.ReadString('\n')
		if err != nil {
			return nil, err
		}
		field, _, _ := strings.Cut(strings.TrimSpace(line), ";")
		size, err := strconv.ParseInt(field, 16, 64)
		if err != nil {
			return nil, err
		}
		if size == 0 {
			_, _ = io.Copy(io.Discard, reader)
			return body, nil
		}
		chunk := make([]byte, size+2)
		if _, err := io.ReadFull(reader, chunk); err != nil {
			return nil, err
		}
		body = append(body, chunk[:size]...)
	}
}

func TestRemoteBackupTargets(t *testing.T) {
	profile, files := fixtureProfile(t)
	passphrase := protocol.NewSecret(fixturePassphrase)
	identity, secret := protocol.NewSecret(fixtureIdentity), protocol.NewSecret(fixtureSecret)
	auth := Auth{Identity: &identity, Secret: &secret, Passphrase: &passphrase}

	dav, davDirectory := fixtureWebDAV(t)
	objects := &fixtureS3{bucket: "profile-backups", objects: map[string][]byte{}}
	s3 := httptest.NewServer(objects)
	t.Cleanup(s3.Close)

	// One sealed archive serves every destination: publishing reads it and
	// leaves it as it was. The plain archive exists only in memory.
	snapshot := fixtureSnapshot(t, profile, files, fixtureRoots)
	var archived bytes.Buffer
	if err := snapshot.archive(t.Context(), &archived); err != nil {
		t.Fatal(err)
	}
	plain := archived.Bytes()
	packed := fixturePack(t, snapshot)
	original, err := os.ReadFile(packed.Path)
	if err != nil {
		t.Fatal(err)
	}

	for _, test := range []struct {
		name        string
		destination protocol.BackupDestination
		stored      func(id string) (marker bool, segments int)
	}{
		{"webdav", protocol.BackupDestination{Kind: protocol.BackupKindWebdav, Endpoint: dav.URL, Root: "team/profile"}, func(id string) (bool, int) {
			_, err := os.Stat(davDirectory + "/team/profile/" + id + ".json")
			segments, _ := os.ReadDir(davDirectory + "/team/profile/" + id)
			return err == nil, len(segments)
		}},
		{"webdav without a prefix", protocol.BackupDestination{Kind: protocol.BackupKindWebdav, Endpoint: dav.URL}, func(id string) (bool, int) {
			_, err := os.Stat(davDirectory + "/" + id + ".json")
			segments, _ := os.ReadDir(davDirectory + "/" + id)
			return err == nil, len(segments)
		}},
		{"s3", protocol.BackupDestination{Kind: protocol.BackupKindS3, Endpoint: s3.URL, Root: "team/profile", Bucket: objects.bucket, Region: "us-east-1"}, func(id string) (bool, int) {
			objects.mu.Lock()
			defer objects.mu.Unlock()
			segments := 0
			for key := range objects.objects {
				if strings.HasPrefix(key, "team/profile/"+id+"/") && strings.HasSuffix(key, ".bin") {
					segments++
				}
			}
			_, marker := objects.objects["team/profile/"+id+".json"]
			return marker, segments
		}},
	} {
		t.Run(test.name, func(t *testing.T) {
			plan := Plan{Destination: &test.destination, OnlineEnabled: true, Auth: auth, Transport: http.DefaultTransport}
			if page, err := List(t.Context(), plan); err != nil || len(page.Records) != 0 || page.Partial {
				t.Fatalf("empty destination = %#v %v", page, err)
			}
			id := uuid.NewString()
			record, err := Export(t.Context(), plan, packed, id)
			if err != nil {
				t.Fatal(err)
			}
			if marker, segments := test.stored(id); !marker || segments != 2 {
				t.Fatalf("stored layout: marker %v, %d segments", marker, segments)
			}
			page, err := List(t.Context(), plan)
			if err != nil || len(page.Records) != 1 || page.Records[0] != record || page.Locations[id] != (protocol.BackupLocations{Online: true}) {
				t.Fatalf("list = %#v %v", page, err)
			}
			fetched, sealed, err := Fetch(t.Context(), plan, id, protocol.BackupLocationOnline, t.TempDir())
			if err != nil || fetched != record {
				t.Fatalf("fetch = %#v %v", fetched, err)
			}
			defer sealed.Close()
			if downloaded, err := io.ReadAll(sealed); err != nil || !bytes.Equal(downloaded, original) {
				t.Fatalf("fetched archive differs from the sealed one: %v", err)
			}
			if _, err := sealed.Seek(0, io.SeekStart); err != nil {
				t.Fatal(err)
			}
			opened, err := Open(t.Context(), sealed, passphrase)
			if err != nil {
				t.Fatal(err)
			}
			if restored, err := io.ReadAll(opened); err != nil || !bytes.Equal(restored, plain) {
				t.Fatalf("restored archive differs: %v", err)
			}
			manifest, err := Unpack(t.Context(), bytes.NewReader(plain), t.TempDir())
			if err != nil || len(manifest.Files) != int(record.Files) {
				t.Fatalf("unpack = %v", err)
			}
			if _, _, err := Fetch(t.Context(), plan, uuid.NewString(), protocol.BackupLocationOnline, t.TempDir()); code(err) != protocol.ErrorCodeNotFound {
				t.Fatalf("unknown backup: %v", err)
			}

			// Rejected credentials are a permission fault that names no secret.
			wrong := protocol.NewSecret("another-secret-value")
			denied := plan
			denied.Auth.Secret = &wrong
			if test.destination.Kind == protocol.BackupKindS3 {
				other := protocol.NewSecret("another-user")
				denied.Auth.Identity = &other
			}
			_, err = List(t.Context(), denied)
			if code(err) != protocol.ErrorCodePermissionDenied || strings.Contains(err.Error(), wrong.Expose()) || strings.Contains(err.Error(), "http") {
				t.Fatalf("rejected credentials: %v", err)
			}
		})
	}

	t.Run("redirects and failures", func(t *testing.T) {
		var redirected bool
		target := httptest.NewServer(http.HandlerFunc(func(http.ResponseWriter, *http.Request) { redirected = true }))
		t.Cleanup(target.Close)
		redirect := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
			http.Redirect(writer, request, target.URL+request.URL.Path, http.StatusTemporaryRedirect)
		}))
		t.Cleanup(redirect.Close)
		failing := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, _ *http.Request) {
			http.Error(writer, "failure", http.StatusInternalServerError)
		}))
		t.Cleanup(failing.Close)
		for _, test := range []struct {
			name        string
			destination protocol.BackupDestination
		}{
			{"webdav redirect", protocol.BackupDestination{Kind: protocol.BackupKindWebdav, Endpoint: redirect.URL}},
			{"s3 redirect", protocol.BackupDestination{Kind: protocol.BackupKindS3, Endpoint: redirect.URL, Bucket: "bucket", Region: "us-east-1"}},
			{"webdav failure", protocol.BackupDestination{Kind: protocol.BackupKindWebdav, Endpoint: failing.URL}},
			{"s3 failure", protocol.BackupDestination{Kind: protocol.BackupKindS3, Endpoint: failing.URL, Bucket: "bucket", Region: "us-east-1"}},
		} {
			plan := Plan{Destination: &test.destination, OnlineEnabled: true, Auth: auth, Transport: http.DefaultTransport}
			_, err := List(t.Context(), plan)
			if code(err) != protocol.ErrorCodeUnavailable || err.Error() != (&protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "backup storage request failed"}).Error() {
				t.Errorf("%s: %v", test.name, err)
			}
			if _, err := Export(t.Context(), plan, packed, uuid.NewString()); err == nil || strings.Contains(err.Error(), fixtureSecret) {
				t.Errorf("%s export: %v", test.name, err)
			}
		}
		if redirected {
			t.Fatal("a storage request followed a redirect")
		}
	})
}
