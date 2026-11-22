package wsl

import (
	"io"
	"net/http"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/identity"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type releaseTransport func(*http.Request) (*http.Response, error)

func (transport releaseTransport) RoundTrip(request *http.Request) (*http.Response, error) {
	return transport(request)
}

func TestForkSelectsPublishedOfficialHost(t *testing.T) {
	const officialVersion = "4.5.6"
	fork := identity.Desktop{Version: "9.0.0", HostRelease: identity.OfficialHost()}
	for _, fixture := range []struct {
		name, body string
		accepted   bool
	}{
		{"published", `{"tag_name":"v` + officialVersion + `","draft":false,"prerelease":false}`, true},
		{"draft", `{"tag_name":"v4.5.6","draft":true}`, false},
		{"prerelease", `{"tag_name":"v4.5.6-beta.1"}`, false},
		{"short tag", `{"tag_name":"v4.5"}`, false},
		{"unprefixed tag", `{"tag_name":"4.5.6"}`, false},
	} {
		t.Run(fixture.name, func(t *testing.T) {
			requested := false
			client := &http.Client{Transport: releaseTransport(func(request *http.Request) (*http.Response, error) {
				requested = true
				if request.Method != http.MethodGet || request.URL.String() != "https://api.github.com/repos/sailry/sailry-harness/releases/latest" {
					t.Fatalf("wrong official repository request: %s %s", request.Method, request.URL)
				}
				return &http.Response{StatusCode: http.StatusOK, Body: io.NopCloser(strings.NewReader(fixture.body))}, nil
			})}
			version, err := resolveRelease(t.Context(), identity.HostRelease{}, client)
			if err != nil || version != link.ProductVersion || requested {
				t.Fatalf("bundled setup changed: version %q, requested %v, %v", version, requested, err)
			}
			version, err = resolveRelease(t.Context(), fork.HostRelease, client)
			if !requested || (err == nil) != fixture.accepted {
				t.Fatalf("official release: version %q, requested %v, %v", version, requested, err)
			}
			if !fixture.accepted {
				return
			}
			script, err := ReleaseScript(version, false, nil)
			if err != nil || version != officialVersion || !strings.Contains(script, "install_version='"+officialVersion+"'") ||
				!strings.Contains(script, "install_downloads='https://github.com/sailry/sailry-harness/releases/download'") || strings.Contains(script, fork.Version) {
				t.Fatalf("release bootstrap did not use the official Host source and version: %q, %v", version, err)
			}
			current := uint16(protocol.VERSION)
			if err := (link.PeerInfo{SoftwareVersion: &version, Protocol: &current}).Compatibility(); err != nil {
				t.Fatalf("matching protocol refused another release version: %v", err)
			}
			current++
			if err := (link.PeerInfo{SoftwareVersion: &version, Protocol: &current}).Compatibility(); err == nil {
				t.Fatal("incompatible protocol was accepted")
			}
		})
	}
}

func TestIndependentHostSelectsItsRelease(t *testing.T) {
	release, err := identity.IndependentHost("stage3-fixture/fork-host", "fixture", "org.fixture.host")
	if err != nil {
		t.Fatal(err)
	}
	client := &http.Client{Transport: releaseTransport(func(request *http.Request) (*http.Response, error) {
		if request.Method != http.MethodGet || request.URL.String() != "https://api.github.com/repos/stage3-fixture/fork-host/releases/latest" {
			t.Fatalf("wrong independent release request: %s %s", request.Method, request.URL)
		}
		return &http.Response{StatusCode: http.StatusOK, Body: io.NopCloser(strings.NewReader(`{"tag_name":"v7.9.0"}`))}, nil
	})}
	if version, err := resolveRelease(t.Context(), release, client); err != nil || version != "7.9.0" {
		t.Fatalf("independent Host release = %q, %v", version, err)
	}
}
