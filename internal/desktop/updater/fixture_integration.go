//go:build integration

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
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/Masterminds/semver/v3"
)

// FixtureTarget is the release target the fixture feed publishes for.
const FixtureTarget = "aarch64-apple-darwin"

// FixtureFeed is a local release source signed with a generated test key,
// for the integration tests of the packages that show the updater. It
// builds the same catalog, signature block and unsigned bundle as this
// package's own test fixture; a service bound to it never reaches the
// official source, the installed application or the system's code-signature
// check.
type FixtureFeed struct {
	Server   *httptest.Server
	public   ed25519.PublicKey
	private  ed25519.PrivateKey
	mu       sync.Mutex
	releases []listedRelease
	files    map[string][]byte
	hits     map[string]int
	stalls   map[string]*fixtureStall
}

type fixtureStall struct {
	reached chan struct{}
	release chan struct{}
}

// NewFixtureFeed serves an empty feed on the loopback interface. The caller
// closes Server.
func NewFixtureFeed() *FixtureFeed {
	// A fixed seed keeps the fixture deterministic.
	private := ed25519.NewKeyFromSeed(bytes.Repeat([]byte{7}, ed25519.SeedSize))
	f := &FixtureFeed{private: private, public: private.Public().(ed25519.PublicKey), files: map[string][]byte{}, hits: map[string]int{}, stalls: map[string]*fixtureStall{}}
	f.Server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		f.mu.Lock()
		f.hits[r.URL.Path]++
		stall := f.stalls[r.URL.Path]
		delete(f.stalls, r.URL.Path)
		var data []byte
		found := true
		if r.URL.Path == "/releases" {
			data, _ = json.Marshal(f.releases)
		} else {
			data, found = f.files[r.URL.Path]
		}
		f.mu.Unlock()
		if !found {
			http.NotFound(w, r)
			return
		}
		half := 0
		if stall != nil {
			// Half of the body arrives in two parts, a download's progress
			// interval apart so the second is reported, then the response
			// waits.
			half = len(data) / 2
			w.Header().Set("Content-Length", fmt.Sprint(len(data)))
			for _, part := range [][]byte{data[:half/2], data[half/2 : half]} {
				_, _ = w.Write(part)
				if flusher, ok := w.(http.Flusher); ok {
					flusher.Flush()
				}
				time.Sleep(150 * time.Millisecond)
			}
			close(stall.reached)
			select {
			case <-stall.release:
			case <-r.Context().Done():
				return
			}
		}
		_, _ = w.Write(data[half:])
	}))
	return f
}

// Stall holds the next response for path after half of its body until
// release is called; reached closes when the response waits.
func (f *FixtureFeed) Stall(path string) (reached <-chan struct{}, release func()) {
	stall := &fixtureStall{reached: make(chan struct{}), release: make(chan struct{})}
	f.mu.Lock()
	f.stalls[path] = stall
	f.mu.Unlock()
	var once sync.Once
	return stall.reached, func() { once.Do(func() { close(stall.release) }) }
}

// Hits counts the requests for path.
func (f *FixtureFeed) Hits(path string) int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.hits[path]
}

// ArchivePath is the feed's path of a version's archive for FixtureTarget.
func (f *FixtureFeed) ArchivePath(version string) string {
	return "/download/v" + version + "/Sailry-" + FixtureTarget + ".zip"
}

// Publish adds a release with its signed catalog and signed archives. A
// damaged release serves an archive that differs from its signed digest.
func (f *FixtureFeed) Publish(version string, damaged bool) {
	tag := "v" + version
	downloads := f.Server.URL + "/download/" + tag
	listed := listedRelease{TagName: tag, Prerelease: semver.MustParse(version).Prerelease() != ""}
	asset := func(name string) {
		listed.Assets = append(listed.Assets, listedAsset{Name: name, State: "uploaded", Size: 1, BrowserDownloadURL: downloads + "/" + name})
	}
	asset("update-v1.json")
	var signed payload
	files := map[string][]byte{}
	for _, target := range []string{"aarch64-apple-darwin", "x86_64-apple-darwin"} {
		name := "Sailry-" + target + ".zip"
		for _, suffix := range []string{".zip", ".zip.update.json", ".dmg"} {
			asset("Sailry-" + target + suffix)
		}
		asset("SHA256SUMS-desktop-" + target)
		archive := fixtureSigned(fixtureArchive(target, version), name, f.private)
		digest := sha256.Sum256(archive)
		if damaged {
			archive = append([]byte{}, archive...)
			archive[len(archive)-1] ^= 1
		}
		files["/download/"+tag+"/"+name] = archive
		signed.Releases = append(signed.Releases, Release{Version: version, Target: target, MinimumSystem: "12.0", Name: name,
			URL: downloads + "/" + name, SHA256: hex.EncodeToString(digest[:]), Size: uint64(len(archive)), Bundle: "Sailry.app", Executable: "Contents/MacOS/sailry-desktop"})
	}
	body := fixtureMust(json.Marshal(signed))
	files["/download/"+tag+"/update-v1.json"] = fixtureMust(json.Marshal(envelope{Version: 1, Payload: body, Signature: base64.StdEncoding.EncodeToString(ed25519.Sign(f.private, body))}))
	f.mu.Lock()
	defer f.mu.Unlock()
	for path, data := range files {
		f.files[path] = data
	}
	f.releases = append(f.releases, listed)
}

// Bind points a service at the feed, at the fixture bundle installed as the
// application, and at a relaunch that only records its executable. The
// fixture bundles are unsigned, so the system's code-signature check is
// replaced until restore is called.
func (f *FixtureFeed) Bind(s *Service, installed string, relaunched func(executable string)) (restore func()) {
	s.release = func(channel Channel, version string) (Config, error) {
		return Config{Source: f.Server.URL + "/releases", Downloads: f.Server.URL + "/download", Channel: channel,
			Keys: []ed25519.PublicKey{f.public}, Target: FixtureTarget, Version: version, System: "14.2", local: true}, nil
	}
	s.installed = func() (string, error) { return installed, nil }
	s.relaunch = func(executable string, _ []string) error {
		relaunched(executable)
		return nil
	}
	previous := verifyCodeSignature
	verifyCodeSignature = func(string) error { return nil }
	return func() { verifyCodeSignature = previous }
}

// WriteFixtureBundle writes an unsigned application bundle of a version.
func WriteFixtureBundle(bundle, version string) error {
	for name, data := range fixtureBundle(FixtureTarget, version) {
		path := filepath.Join(bundle, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
			return err
		}
		if err := os.WriteFile(path, data, 0o755); err != nil {
			return err
		}
	}
	return nil
}

// FixtureBundleVersion is the version a bundle's build metadata names.
func FixtureBundleVersion(bundle string) (string, error) {
	data, err := os.ReadFile(filepath.Join(bundle, "Contents", "Resources", "build.json"))
	if err != nil {
		return "", err
	}
	var value build
	if err := json.Unmarshal(data, &value); err != nil {
		return "", err
	}
	return value.ApplicationVersion, nil
}

func fixtureMust(data []byte, err error) []byte {
	if err != nil {
		panic(err)
	}
	return data
}

// fixtureSigned prepends the ed25519ph signature block a release carries.
func fixtureSigned(archive []byte, name string, key ed25519.PrivateKey) []byte {
	digest := sha512.Sum512(archive)
	signature := fixtureMust(key.Sign(nil, digest[:], &ed25519.Options{Hash: crypto.SHA512, Context: name}))
	signed := append([]byte{}, signatureMagic...)
	signed = binary.LittleEndian.AppendUint16(signed, 1)
	signed = append(signed, signature...)
	return append(signed, archive...)
}

func fixtureBundle(target, version string) map[string][]byte {
	parsed := semver.MustParse(version)
	plist := fmt.Sprintf(`<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>ai.sailry.desktop</string>
<key>CFBundleExecutable</key><string>sailry-desktop</string>
<key>CFBundleShortVersionString</key><string>%d.%d.%d</string>
<key>LSMinimumSystemVersion</key><string>12.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>`, parsed.Major(), parsed.Minor(), parsed.Patch())
	header := make([]byte, 96)
	binary.LittleEndian.PutUint32(header, 0xfeedfacf)
	cpu := uint32(0x01000007)
	if strings.HasPrefix(target, "aarch64-") {
		cpu = 0x0100000c
	}
	binary.LittleEndian.PutUint32(header[4:], cpu)
	return map[string][]byte{
		"Contents/MacOS/sailry-desktop": header,
		"Contents/Resources/build.json": fixtureMust(json.Marshal(build{Version: 1, Target: target, ApplicationVersion: version})),
		"Contents/Info.plist":           []byte(plist),
	}
}

func fixtureArchive(target, version string) []byte {
	var buffer bytes.Buffer
	writer := zip.NewWriter(&buffer)
	modified := time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)
	files := fixtureBundle(target, version)
	for _, name := range []string{"Contents/Info.plist", "Contents/MacOS/sailry-desktop", "Contents/Resources/build.json"} {
		header := &zip.FileHeader{Name: "Sailry.app/" + name, Method: zip.Deflate, Modified: modified}
		header.SetMode(0o644)
		if strings.Contains(name, "MacOS") {
			header.SetMode(0o755)
		}
		entry, err := writer.CreateHeader(header)
		if err == nil {
			_, err = entry.Write(files[name])
		}
		if err != nil {
			panic(err)
		}
	}
	link := &zip.FileHeader{Name: "Sailry.app/Contents/Resources/current.json", Modified: modified}
	link.SetMode(0o777 | os.ModeSymlink)
	entry, err := writer.CreateHeader(link)
	if err == nil {
		_, err = entry.Write([]byte("build.json"))
	}
	if err == nil {
		err = writer.Close()
	}
	if err != nil {
		panic(err)
	}
	return buffer.Bytes()
}
