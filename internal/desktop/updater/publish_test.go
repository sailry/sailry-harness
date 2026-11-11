package updater

import (
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/Masterminds/semver/v3"
)

// TestPublishedRelease signs a release with a throwaway key and checks it
// with the updater's own verification: the published files are accepted,
// and a changed archive, another key and a version that is not newer are
// refused.
func TestPublishedRelease(t *testing.T) {
	unsignedBundles(t)
	public, private, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatal(err)
	}
	foreignPublic, foreignPrivate, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatal(err)
	}
	keys, foreign := []ed25519.PublicKey{public}, []ed25519.PublicKey{foreignPublic}
	source, output := t.TempDir(), t.TempDir()
	unsigned := map[string]string{}
	for _, target := range []string{"aarch64-apple-darwin", "x86_64-apple-darwin"} {
		unsigned[target] = filepath.Join(source, target+".zip")
		if err := os.WriteFile(unsigned[target], bundleArchive(t, target, "1.1.0", "13.0"), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	if err := publish(foreignPrivate, keys, "1.1.0", t.TempDir(), unsigned); err == nil {
		t.Fatal("a key outside the trusted keys signed a release")
	}
	if err := publish(private, keys, "1.1.0", output, unsigned); err != nil {
		t.Fatal(err)
	}
	catalog, err := os.ReadFile(filepath.Join(output, "update-v1.json"))
	if err != nil {
		t.Fatal(err)
	}
	config := Config{Downloads: Downloads, Channel: Stable, Keys: keys, Target: fixtureTarget, Version: "1.0.0", System: "14.2"}
	key := func(err error) string {
		if err == nil {
			return ""
		}
		return asFailure(err).Key
	}

	// Accepted: discovery's exact-version check, selection, archive and bundle.
	listed := published{version: semver.MustParse("1.1.0"), downloads: Downloads + "/v1.1.0"}
	if err := listed.authenticate(catalog, &config); err != nil {
		t.Fatal(err)
	}
	selection, err := config.selectRelease(catalog)
	if err != nil || selection == nil {
		t.Fatalf("selection = %v, %v", selection, err)
	}
	release := selection.Release
	archive := filepath.Join(output, release.Name)
	if release.Version != "1.1.0" || release.Name != "Sailry-"+fixtureTarget+".zip" || release.MinimumSystem != "13.0" {
		t.Fatalf("release = %+v", release)
	}
	if err := verifyArchive(archive, release, keys); err != nil {
		t.Fatal(err)
	}
	if _, err := extract(archive, release, t.TempDir()); err != nil {
		t.Fatal(err)
	}
	var described Release
	metadata, err := os.ReadFile(archive + ".update.json")
	if err != nil || json.Unmarshal(metadata, &described) != nil || described != release {
		t.Fatalf("update metadata = %s, %v", metadata, err)
	}

	// Refused: a changed archive, with the signed digest and with its own.
	data, err := os.ReadFile(archive)
	if err != nil {
		t.Fatal(err)
	}
	data[len(data)-1] ^= 1
	tampered := filepath.Join(t.TempDir(), release.Name)
	if err := os.WriteFile(tampered, data, 0o600); err != nil {
		t.Fatal(err)
	}
	if got := key(verifyArchive(tampered, release, keys)); got != "updates_digest_invalid" {
		t.Fatalf("tampered archive = %q", got)
	}
	digest := sha256.Sum256(data)
	redescribed := release
	redescribed.SHA256 = hex.EncodeToString(digest[:])
	if got := key(verifyArchive(tampered, redescribed, keys)); got != "updates_signature_invalid" {
		t.Fatalf("tampered archive with its own digest = %q", got)
	}

	// Refused: another publisher key, for the catalog and for the archive.
	untrusted := config
	untrusted.Keys = foreign
	if _, err := untrusted.selectRelease(catalog); key(err) != "updates_signature_invalid" {
		t.Fatalf("catalog under another key = %v", err)
	}
	if got := key(verifyArchive(archive, release, foreign)); got != "updates_signature_invalid" {
		t.Fatalf("archive under another key = %q", got)
	}

	// Refused: a build that already runs this version or a newer one.
	for _, current := range []string{"1.1.0", "2.0.0"} {
		newer := config
		newer.Version = current
		if selected, err := newer.selectRelease(catalog); selected != nil || err != nil {
			t.Fatalf("version %s selected %v, %v", current, selected, err)
		}
		if got := key(newer.revalidate(selection)); got != "updates_version_old" {
			t.Fatalf("version %s revalidation = %q", current, got)
		}
	}
}
