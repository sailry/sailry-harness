package updater

import (
	"net/http"
	"os"
	"path/filepath"
	"slices"
	"sync"
	"testing"
)

// TestUpdaterStateMapping drives the state machine against a local signed
// feed: which state each outcome ends in and which notice or failure key it
// reports, per channel.
func TestUpdaterStateMapping(t *testing.T) {
	unsignedBundles(t)
	stable := []fixtureRelease{{version: "1.1.0"}, {version: "1.2.0-beta.1"}}
	tests := []struct {
		name     string
		current  string
		channel  Channel
		required string
		releases []fixtureRelease
		status   int
		check    expectation
		download *expectation
	}{
		{name: "stable ignores prereleases", current: "1.0.0", channel: Stable, releases: stable,
			check: expectation{kind: Available, version: "1.1.0"}, download: &expectation{kind: Ready, version: "1.1.0", notice: "updates_ready"}},
		{name: "experience takes the newest", current: "1.0.0", channel: Experience, releases: stable,
			check: expectation{kind: Available, version: "1.2.0-beta.1"}, download: &expectation{kind: Ready, version: "1.2.0-beta.1", notice: "updates_ready"}},
		{name: "current build", current: "1.1.0", channel: Stable, releases: stable, check: expectation{kind: Idle, notice: "updates_current"}},
		{name: "prerelease build on stable waits for its release", current: "1.2.0-beta.1", channel: Stable, releases: stable, check: expectation{kind: Idle, notice: "updates_current"}},
		{name: "no releases", current: "1.0.0", channel: Stable, check: expectation{kind: Idle, notice: "updates_current"}},
		{name: "release still uploading", current: "1.0.0", channel: Stable, releases: []fixtureRelease{{version: "1.1.0", missingAsset: true}},
			check: expectation{kind: Idle, notice: "updates_current"}},
		{name: "source refuses", current: "1.0.0", channel: Stable, releases: stable, status: http.StatusInternalServerError,
			check: expectation{kind: Idle, notice: "updates_source_unready", failure: "updates_source_unready"}},
		{name: "untrusted catalog", current: "1.0.0", channel: Stable, releases: []fixtureRelease{{version: "1.1.0", foreignCatalog: true}},
			check: expectation{kind: Idle, notice: "updates_signature_invalid", failure: "updates_signature_invalid"}},
		{name: "assets outside the release", current: "1.0.0", channel: Stable, releases: []fixtureRelease{{version: "1.1.0", mismatchedAssets: true}},
			check: expectation{kind: Idle, notice: "updates_manifest_invalid", failure: "updates_manifest_invalid"}},
		{name: "system too old", current: "1.0.0", channel: Stable, releases: []fixtureRelease{{version: "1.1.0", minimum: "15.0"}},
			check: expectation{kind: Idle, notice: "updates_system_old", failure: "updates_system_old"}},
		{name: "no package for this target", current: "1.0.0", channel: Stable, releases: []fixtureRelease{{version: "1.1.0", otherTarget: true}},
			check: expectation{kind: Idle, notice: "updates_platform", failure: "updates_platform"}},
		{name: "required version", current: "1.0.0", channel: Stable, required: "1.2.0-beta.1", releases: stable,
			check: expectation{kind: Available, version: "1.2.0-beta.1"}},
		{name: "required version unpublished", current: "1.0.0", channel: Stable, required: "1.3.0", releases: stable,
			check: expectation{kind: Idle, notice: "compatibility_update_unavailable", failure: "compatibility_update_unavailable"}},
		{name: "damaged archive", current: "1.0.0", channel: Stable, releases: []fixtureRelease{{version: "1.1.0", damagedArchive: true}},
			check:    expectation{kind: Available, version: "1.1.0"},
			download: &expectation{kind: Available, version: "1.1.0", notice: "updates_digest_invalid", failure: "updates_digest_invalid"}},
		{name: "untrusted archive", current: "1.0.0", channel: Stable, releases: []fixtureRelease{{version: "1.1.0", foreignArchive: true}},
			check:    expectation{kind: Available, version: "1.1.0"},
			download: &expectation{kind: Available, version: "1.1.0", notice: "updates_signature_invalid", failure: "updates_signature_invalid"}},
		{name: "bundle of another version", current: "1.0.0", channel: Stable, releases: []fixtureRelease{{version: "1.1.0", wrongBundle: true}},
			check:    expectation{kind: Available, version: "1.1.0"},
			download: &expectation{kind: Available, version: "1.1.0", notice: "updates_package_invalid", failure: "updates_package_invalid"}},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			source := newFeed(t)
			for _, release := range test.releases {
				source.publish(release)
			}
			source.status = test.status
			root := filepath.Join(t.TempDir(), "updater")
			// Every change the interface is woken for, as it would read it.
			var watch sync.Mutex
			var seen []State
			var service *Service
			service = New(Options{Version: test.current, Directory: root, Channel: func() Channel { return test.channel }, Changed: func() {
				state, _ := service.Snapshot()
				watch.Lock()
				seen = append(seen, state)
				watch.Unlock()
			}})
			service.release = func(channel Channel, version string) (Config, error) { return source.config(channel, version), nil }
			t.Cleanup(service.Close)
			expect := func(step string, want expectation) {
				t.Helper()
				state, failure, notices := settle(service)
				if state.Kind != want.kind || state.Version != want.version {
					t.Fatalf("%s state = %+v, want %+v", step, state, want)
				}
				key := ""
				if failure != nil {
					key = failure.Key
				}
				if key != want.failure {
					t.Fatalf("%s failure = %q (%v), want %q", step, key, failure, want.failure)
				}
				if want.notice == "" && len(notices) != 0 || want.notice != "" && (len(notices) != 1 || notices[0].Key != want.notice || notices[0].Error != (want.failure != "")) {
					t.Fatalf("%s notices = %+v, want %q", step, notices, want.notice)
				}
			}
			if test.required != "" {
				service.CheckVersion(test.required)
			} else {
				service.Check()
			}
			if state, _ := service.Snapshot(); state.Kind != Checking {
				t.Fatalf("check started in %+v", state)
			}
			expect("check", test.check)
			if test.download == nil {
				service.Download()
				if test.check.kind != Available {
					expect("download without a release", test.check.without())
				}
				return
			}
			service.Download()
			expect("download", *test.download)
			entries, _ := os.ReadDir(root)
			if staged := test.download.kind == Ready; staged != (len(entries) == 1) {
				t.Fatalf("staging holds %d entries after %+v", len(entries), *test.download)
			}
			if test.download.kind != Ready {
				return
			}
			// SABOUT-005: the download reports the bytes copied up to the
			// signed size, then verifies, before it is ready.
			watch.Lock()
			var progress []Progress
			for _, state := range seen {
				if state.Kind == Downloading {
					progress = append(progress, state.Progress)
				}
			}
			final := seen[len(seen)-1]
			watch.Unlock()
			if len(progress) < 2 || final.Kind != Ready {
				t.Fatalf("download reported %+v and ended in %+v", progress, final)
			}
			for index, step := range progress {
				if step.Total == 0 || step.Copied > step.Total || index > 0 && (step.Copied < progress[index-1].Copied || progress[index-1].Verifying && !step.Verifying) {
					t.Fatalf("download progress %d of %+v", index, progress)
				}
			}
			if last := progress[len(progress)-1]; !last.Verifying || last.Copied != last.Total ||
				!slices.ContainsFunc(progress, func(step Progress) bool { return !step.Verifying && step.Copied == step.Total }) {
				t.Fatalf("download ended without the full size or without verifying: %+v", progress)
			}
			// A channel change abandons the staged release and asks for a new check.
			if !service.ChannelChanged() {
				t.Fatal("a channel change with a staged release asked for no check")
			}
			state, failure, notices := settle(service)
			if entries, _ := os.ReadDir(root); state.Kind != Idle || failure != nil || len(notices) != 0 || len(entries) != 0 {
				t.Fatalf("after channel change: %+v %v %v, %d staged", state, failure, notices, len(entries))
			}
		})
	}
	t.Run("only an idle or available state checks", func(t *testing.T) {
		source := newFeed(t)
		source.publish(fixtureRelease{version: "1.1.0"})
		service := New(Options{Version: "1.0.0", Directory: filepath.Join(t.TempDir(), "updater"), Channel: func() Channel { return Stable }})
		service.release = func(channel Channel, version string) (Config, error) { return source.config(channel, version), nil }
		t.Cleanup(service.Close)
		for _, action := range []func(){service.Download, service.Install, service.Restart, service.Cancel} {
			action()
			if state, failure, notices := settle(service); state.Kind != Idle || failure != nil || len(notices) != 0 {
				t.Fatalf("an idle service moved to %+v %v %v", state, failure, notices)
			}
		}
		if service.ChannelChanged() {
			t.Fatal("an idle, unchecked service asked for a check")
		}
		service.Check()
		settle(service)
		service.Download()
		settle(service)
		lists := source.hit("/releases")
		service.Check()
		service.Download()
		if state, _, _ := settle(service); state.Kind != Ready || source.hit("/releases") != lists {
			t.Fatalf("a ready service checked again: %+v", state)
		}
	})
}

// expectation is the state an action ends in and what it reports.
type expectation struct {
	kind    Kind
	version string
	notice  string // the single notice's key, empty for none
	failure string // the recorded failure's key, empty for none
}

// without is the expectation after an action the state refuses.
func (e expectation) without() expectation {
	e.notice = ""
	return e
}
