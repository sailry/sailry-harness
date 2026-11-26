package updater

import (
	"archive/zip"
	"bytes"
	"crypto/ed25519"
	"encoding/base64"
	"encoding/binary"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/identity"
)

func TestForkReleaseUsesItsOwnSourceTrustAndPackage(t *testing.T) {
	unsignedBundles(t)
	const version = "7.1.0"
	const releasePath = "/fork/desktop/download/v" + version + "/"
	output := t.TempDir()
	var listing []listedRelease
	server := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case r.URL.Path == "/fork/desktop/releases":
			_ = json.NewEncoder(w).Encode(listing)
		case strings.HasPrefix(r.URL.Path, releasePath):
			http.ServeFile(w, r, filepath.Join(output, filepath.Base(r.URL.Path)))
		default:
			http.NotFound(w, r)
		}
	}))
	t.Cleanup(server.Close)

	seed := bytes.Repeat([]byte{41}, ed25519.SeedSize)
	public := ed25519.NewKeyFromSeed(seed).Public().(ed25519.PublicKey)
	fork := identity.DesktopRelease{
		Package:    identity.DesktopPackage{Name: "FixtureDesktop", ID: "test.fixture.desktop", Executable: "fixture-desktop"},
		Source:     server.URL + "/fork/desktop/releases",
		Downloads:  server.URL + "/fork/desktop/download",
		PublicKeys: base64.StdEncoding.EncodeToString(public),
	}
	source := t.TempDir()
	archives := map[string]string{}
	for _, target := range []string{"aarch64-apple-darwin", "x86_64-apple-darwin"} {
		archives[target] = filepath.Join(source, target+".zip")
		if err := os.WriteFile(archives[target], bundleArchiveFor(t, target, version, "13.0", fork.Package), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	const windowsTarget = "x86_64-pc-windows-msvc"
	archives[windowsTarget] = filepath.Join(source, windowsTarget+".zip")
	if err := os.WriteFile(archives[windowsTarget], windowsBundleArchive(t, fork.Package.Name, version), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := PublishRelease(fork, seed, version, output, archives); err != nil {
		t.Fatal(err)
	}

	listed := listedRelease{TagName: "v" + version}
	add := func(name string) {
		listed.Assets = append(listed.Assets, listedAsset{Name: name, State: "uploaded", Size: 1, BrowserDownloadURL: server.URL + releasePath + name})
	}
	add("update-v1.json")
	add("update-v1-windows.json")
	for target := range archives {
		if strings.HasSuffix(target, "apple-darwin") {
			add(fork.Package.Name + "-" + target + ".dmg")
		}
		for _, suffix := range []string{".zip", ".zip.update.json"} {
			add(fork.Package.Name + "-" + target + suffix)
		}
		add("SHA256SUMS-desktop-" + target)
	}
	listing = []listedRelease{listed}

	config, err := releaseInputs(fork)
	if err != nil {
		t.Fatal(err)
	}
	config.Target, config.Version, config.System = fixtureTarget, "7.0.0", "14.2"
	selection, err := check(t.Context(), server.Client(), &config)
	if err != nil || selection == nil || selection.Release.Version != version || selection.Release.Bundle != "FixtureDesktop.app" {
		t.Fatalf("fork selection = %v, %v", selection, err)
	}
	stage, err := download(t.Context(), server.Client(), &config, selection, t.TempDir(), func(Progress) {})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(filepath.Join(stage.directory, "contents", "FixtureDesktop.app", "Contents", "MacOS", "fixture-desktop")); err != nil {
		t.Fatal(err)
	}

	// Older macOS clients validate all entries using their original package layout.
	mac, err := config.parse(selection.Manifest)
	if err != nil || len(mac) != 2 || !strings.HasSuffix(mac[0].Target, "apple-darwin") || !strings.HasSuffix(mac[1].Target, "apple-darwin") {
		t.Fatalf("macOS catalog = %+v, %v", mac, err)
	}
	windows := config
	windows.Target, windows.System = windowsTarget, "10.0.19045"
	selected, err := check(t.Context(), server.Client(), &windows)
	if err != nil || selected == nil || selected.Release.Executable != "FixtureDesktop.exe" || selected.Release.MinimumSystem != "10.0" {
		t.Fatalf("Windows x64 selection = %+v, %v", selected, err)
	}
	staged, err := download(t.Context(), server.Client(), &windows, selected, t.TempDir(), func(Progress) {})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(filepath.Join(staged.directory, "contents", "FixtureDesktop", "FixtureDesktop.exe")); err != nil {
		t.Fatal(err)
	}

	official := identity.OfficialDesktop().Release
	official.Package = fork.Package
	official.Source, official.Downloads = fork.Source, fork.Downloads
	otherKeys, err := releaseInputs(official)
	if err != nil {
		t.Fatal(err)
	}
	otherKeys.Target, otherKeys.Version, otherKeys.System = config.Target, config.Version, config.System
	if _, err := check(t.Context(), server.Client(), &otherKeys); err == nil || asFailure(err).Key != "updates_signature_invalid" {
		t.Fatalf("official key accepted a fork catalog: %v", err)
	}
	wrongBundle := config
	wrongBundle.Package.ID = identity.OfficialDesktop().Release.Package.ID
	if _, err := download(t.Context(), server.Client(), &wrongBundle, selection, t.TempDir(), func(Progress) {}); err == nil || asFailure(err).Key != "updates_package_invalid" {
		t.Fatalf("signed archive accepted with the wrong expected bundle identifier: %v", err)
	}

	if runtime.GOOS == "darwin" || runtime.GOOS == "windows" && runtime.GOARCH == "amd64" {
		service := New(Options{Version: "7.0.0", Release: fork})
		defer service.Close()
		service.client = func(*Config) *http.Client { return server.Client() }
		service.Check()
		state, failure, _ := settle(service)
		if failure != nil || state.Kind != Available || state.Version != version {
			t.Fatalf("fork service did not check the configured source: %v, %v", state, failure)
		}
	}
}

// windowsBundleArchive models the PE header and root-level build record shipped by package.sh.
func windowsBundleArchive(t *testing.T, name, version string) []byte {
	t.Helper()
	header := make([]byte, 70)
	copy(header, "MZ")
	binary.LittleEndian.PutUint32(header[60:], 64)
	copy(header[64:], "PE\x00\x00")
	binary.LittleEndian.PutUint16(header[68:], 0x8664)
	metadata, err := json.Marshal(build{Version: 1, Target: "x86_64-pc-windows-msvc", ApplicationVersion: version})
	if err != nil {
		t.Fatal(err)
	}
	var buffer bytes.Buffer
	archive := zip.NewWriter(&buffer)
	for _, file := range []struct {
		path string
		data []byte
	}{{name + ".exe", header}, {windowsHelper, header}, {"build.json", metadata}} {
		entry, err := archive.Create(name + "/" + file.path)
		if err != nil {
			t.Fatal(err)
		}
		if _, err := entry.Write(file.data); err != nil {
			t.Fatal(err)
		}
	}
	if err := archive.Close(); err != nil {
		t.Fatal(err)
	}
	return buffer.Bytes()
}
