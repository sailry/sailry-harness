package updater

import (
	"archive/zip"
	"bytes"
	"crypto"
	"crypto/ed25519"
	"crypto/sha256"
	"crypto/sha512"
	"encoding/base64"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"runtime"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/Masterminds/semver/v3"
)

const fixtureTarget = "aarch64-apple-darwin"

// feed is a local release source signed with a generated test key.
type feed struct {
	t        *testing.T
	server   *httptest.Server
	public   ed25519.PublicKey
	private  ed25519.PrivateKey
	mu       sync.Mutex
	releases []listedRelease
	files    map[string][]byte
	status   int
	hits     map[string]int
}

// published tunes one fixture release.
type fixtureRelease struct {
	version          string
	minimum          string
	foreignCatalog   bool // the catalog is signed by an untrusted key
	foreignArchive   bool // the archive is signed by an untrusted key
	damagedArchive   bool // the served archive differs from its signed digest
	otherTarget      bool // the catalog has no package for this target
	wrongBundle      bool // the bundle's build metadata names another version
	missingAsset     bool // the release is still uploading
	mismatchedAssets bool // an asset's URL is outside the release
}

func newFeed(t *testing.T) *feed {
	t.Helper()
	// A fixed seed keeps the fixture deterministic.
	private := ed25519.NewKeyFromSeed(bytes.Repeat([]byte{7}, ed25519.SeedSize))
	f := &feed{t: t, private: private, public: private.Public().(ed25519.PublicKey), files: map[string][]byte{}, hits: map[string]int{}}
	f.server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		f.mu.Lock()
		defer f.mu.Unlock()
		f.hits[r.URL.Path]++
		if f.status != 0 {
			w.WriteHeader(f.status)
			return
		}
		if r.URL.Path == "/releases" {
			if r.URL.Query().Get("per_page") != "100" || r.URL.Query().Get("page") != "1" {
				http.Error(w, "unexpected page", http.StatusBadRequest)
				return
			}
			_ = json.NewEncoder(w).Encode(f.releases)
			return
		}
		data, ok := f.files[r.URL.Path]
		if !ok {
			http.NotFound(w, r)
			return
		}
		_, _ = w.Write(data)
	}))
	t.Cleanup(f.server.Close)
	return f
}

func (f *feed) config(channel Channel, version string) Config {
	return Config{Source: f.server.URL + "/releases", Downloads: f.server.URL + "/download", Channel: channel,
		Keys: []ed25519.PublicKey{f.public}, Target: fixtureTarget, Version: version, System: "14.2", local: true}
}

func (f *feed) hit(path string) int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.hits[path]
}

// publish adds a release with its signed catalog and signed archives.
func (f *feed) publish(release fixtureRelease) {
	f.t.Helper()
	if release.minimum == "" {
		release.minimum = "12.0"
	}
	foreign := ed25519.NewKeyFromSeed(bytes.Repeat([]byte{9}, ed25519.SeedSize))
	tag := "v" + release.version
	downloads := f.server.URL + "/download/" + tag
	listed := listedRelease{TagName: tag, Prerelease: semver.MustParse(release.version).Prerelease() != ""}
	asset := func(name string) {
		url := downloads + "/" + name
		if release.mismatchedAssets {
			url = f.server.URL + "/elsewhere/" + name
		}
		listed.Assets = append(listed.Assets, listedAsset{Name: name, State: "uploaded", Size: 1, BrowserDownloadURL: url})
	}
	asset("update-v1.json")
	var signed payload
	for _, target := range []string{"aarch64-apple-darwin", "x86_64-apple-darwin"} {
		name := "Sailry-" + target + ".zip"
		for _, suffix := range []string{".zip", ".zip.update.json", ".dmg"} {
			if !(release.missingAsset && suffix == ".dmg") {
				asset("Sailry-" + target + suffix)
			}
		}
		asset("SHA256SUMS-desktop-" + target)
		packaged := release.version
		if release.wrongBundle {
			packaged = "0.0.1"
		}
		signer := f.private
		if release.foreignArchive {
			signer = foreign
		}
		archive := signArchive(f.t, bundleArchive(f.t, target, packaged, release.minimum), name, signer)
		digest := sha256.Sum256(archive)
		if release.damagedArchive {
			archive = append([]byte{}, archive...)
			archive[len(archive)-1] ^= 1
		}
		f.files["/download/"+tag+"/"+name] = archive
		if release.otherTarget && target == fixtureTarget {
			continue
		}
		signed.Releases = append(signed.Releases, Release{Version: release.version, Target: target, MinimumSystem: release.minimum, Name: name,
			URL: downloads + "/" + name, SHA256: hex.EncodeToString(digest[:]), Size: uint64(len(archive)), Bundle: "Sailry.app", Executable: "Contents/MacOS/sailry-desktop"})
	}
	signer := f.private
	if release.foreignCatalog {
		signer = foreign
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	f.files["/download/"+tag+"/update-v1.json"] = signCatalog(f.t, signed, signer)
	f.releases = append(f.releases, listed)
}

func signCatalog(t *testing.T, signed payload, key ed25519.PrivateKey) []byte {
	t.Helper()
	body, err := json.Marshal(signed)
	if err != nil {
		t.Fatal(err)
	}
	data, err := json.Marshal(envelope{Version: 1, Payload: body, Signature: base64.StdEncoding.EncodeToString(ed25519.Sign(key, body))})
	if err != nil {
		t.Fatal(err)
	}
	return data
}

// signArchive prepends the ed25519ph signature block a release carries.
func signArchive(t *testing.T, archive []byte, name string, key ed25519.PrivateKey) []byte {
	t.Helper()
	digest := sha512.Sum512(archive)
	signature, err := key.Sign(nil, digest[:], &ed25519.Options{Hash: crypto.SHA512, Context: name})
	if err != nil {
		t.Fatal(err)
	}
	signed := append([]byte{}, signatureMagic...)
	signed = binary.LittleEndian.AppendUint16(signed, 1)
	signed = append(signed, signature...)
	return append(signed, archive...)
}

func executableHeader(target string) []byte {
	header := make([]byte, 96)
	binary.LittleEndian.PutUint32(header, 0xfeedfacf)
	cpu := uint32(0x01000007)
	if strings.HasPrefix(target, "aarch64-") {
		cpu = 0x0100000c
	}
	binary.LittleEndian.PutUint32(header[4:], cpu)
	return header
}

func bundleFiles(target, version, minimum string) map[string][]byte {
	parsed := semver.MustParse(version)
	plist := fmt.Sprintf(`<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>ai.sailry.desktop</string>
<key>CFBundleExecutable</key><string>sailry-desktop</string>
<key>CFBundleShortVersionString</key><string>%d.%d.%d</string>
<key>LSMinimumSystemVersion</key><string>%s</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>`, parsed.Major(), parsed.Minor(), parsed.Patch(), minimum)
	metadata, _ := json.Marshal(build{Version: 1, Target: target, ApplicationVersion: version})
	return map[string][]byte{
		"Contents/MacOS/sailry-desktop": executableHeader(target),
		"Contents/Resources/build.json": metadata,
		"Contents/Info.plist":           []byte(plist),
	}
}

func bundleArchive(t *testing.T, target, version, minimum string) []byte {
	t.Helper()
	var buffer bytes.Buffer
	writer := zip.NewWriter(&buffer)
	modified := time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)
	files := bundleFiles(target, version, minimum)
	for _, name := range []string{"Contents/Info.plist", "Contents/MacOS/sailry-desktop", "Contents/Resources/build.json"} {
		header := &zip.FileHeader{Name: "Sailry.app/" + name, Method: zip.Deflate, Modified: modified}
		header.SetMode(0o644)
		if strings.Contains(name, "MacOS") {
			header.SetMode(0o755)
		}
		entry, err := writer.CreateHeader(header)
		if err != nil {
			t.Fatal(err)
		}
		if _, err := entry.Write(files[name]); err != nil {
			t.Fatal(err)
		}
	}
	resource := &zip.FileHeader{Name: "Sailry.app/Contents/Resources/current.json", Modified: modified}
	content := []byte("build.json")
	if runtime.GOOS == "windows" {
		// Windows does not extract Unix symbolic links. The portable fixture
		// keeps the same metadata in a regular resource instead.
		resource.SetMode(0o644)
		content = files["Contents/Resources/build.json"]
	} else {
		resource.SetMode(0o777 | os.ModeSymlink)
	}
	entry, err := writer.CreateHeader(resource)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := entry.Write(content); err != nil {
		t.Fatal(err)
	}
	if err := writer.Close(); err != nil {
		t.Fatal(err)
	}
	return buffer.Bytes()
}

// settle waits for the service's background work and returns what it shows.
func settle(s *Service) (State, *Failure, []Notice) {
	s.work.Wait()
	state, failure := s.Snapshot()
	return state, failure, s.Notices()
}

// unsignedBundles accepts the fixtures' unsigned bundles for one test.
func unsignedBundles(t *testing.T) {
	t.Helper()
	previous := verifyCodeSignature
	verifyCodeSignature = func(string) error { return nil }
	t.Cleanup(func() { verifyCodeSignature = previous })
}
