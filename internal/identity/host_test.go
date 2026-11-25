package identity

import "testing"

func TestCodeHostRetainsOfficialNamespaceWithItsOwnFeed(t *testing.T) {
	t.Parallel()
	release := CodeHost()
	if !release.Published() || release.Independent() || release.Package() != OfficialHost().Package() {
		t.Fatalf("Code Host did not retain the official installation namespace: %+v", release)
	}
	if release.Releases() != "https://api.github.com/repos/sailry/sailry-code/releases" ||
		release.Downloads() != "https://github.com/sailry/sailry-code/releases/download" {
		t.Fatalf("Code Host source = %q, %q", release.Releases(), release.Downloads())
	}
	if pkg := release.Package(); pkg.Name() != "sailry" || pkg.Binary() != "sailry-host" || pkg.Profile() != ".sailry-host" || pkg.LaunchAgent() != "ai.sailry.host" {
		t.Fatalf("Code Host package = %+v", pkg)
	}
}

func TestIndependentHostCannotClaimOfficialProducts(t *testing.T) {
	t.Parallel()
	for _, repository := range []string{OfficialRepository, "SAILRY/SAILRY-HARNESS", codeHostRepository, "SAILRY/SAILRY-CODE"} {
		if _, err := IndependentHost(repository, "fixture", "org.fixture.host"); err == nil {
			t.Fatalf("independent Host claimed official repository %q", repository)
		}
	}
	for _, namespace := range []struct{ name, agent string }{
		{"sailry", "org.fixture.host"}, {"fixture", "ai.sailry.host"}, {"sailry", "ai.sailry.host"},
	} {
		if _, err := IndependentHost("example/fork", namespace.name, namespace.agent); err == nil {
			t.Fatalf("independent Host claimed official namespace %+v", namespace)
		}
	}
	release, err := IndependentHost("example/fork", "fixture", "org.fixture.host")
	if err != nil || !release.Independent() || release.Package().Name() != "fixture" || release.Downloads() != "https://github.com/example/fork/releases/download" {
		t.Fatalf("independent Host = %+v, %v", release, err)
	}
}
