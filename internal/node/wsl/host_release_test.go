package wsl

import (
	"io"
	"net/http"
	"strconv"
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

func TestCodeHostSelectsItsFeedWithTheExistingNamespace(t *testing.T) {
	t.Parallel()
	product := identity.Desktop{
		Version: "9.0.0", HostRelease: identity.CodeHost(),
		Release: identity.DesktopRelease{Source: "https://desktop.example/feed", Downloads: "https://desktop.example/download"},
	}
	client := &http.Client{Transport: releaseTransport(func(request *http.Request) (*http.Response, error) {
		if request.Method != http.MethodGet || request.URL.String() != "https://api.github.com/repos/sailry/sailry-code/releases?page=1&per_page=100" {
			t.Fatalf("wrong Code Host release request: %s %s", request.Method, request.URL)
		}
		return &http.Response{StatusCode: http.StatusOK, Body: io.NopCloser(strings.NewReader(`[
			{"tag_name":"mobile-v8.0.0+40"},
			{"tag_name":"v4.5.5"},
			{"tag_name":"v4.5.6"},
			{"tag_name":"v9.0.0","draft":true},
			{"tag_name":"v10.0.0","prerelease":true}
		]`))}, nil
	})}
	version, err := resolveRelease(t.Context(), product.HostRelease, client)
	if err != nil || version != "4.5.6" {
		t.Fatalf("Code Host release = %q, %v", version, err)
	}
	for _, update := range []bool{false, true} {
		script, err := ReleaseScriptFor(product.HostRelease, version, update, nil)
		if err != nil || !strings.Contains(script, "install_downloads='https://github.com/sailry/sailry-code/releases/download'") ||
			strings.Contains(script, "sailry-harness") || strings.Contains(script, "desktop.example") || strings.Contains(script, product.Version) {
			t.Fatalf("Code Host bootstrap used another product feed or version: %q, %v", script, err)
		}
	}
	preflight := PreflightScript(product.HostRelease)
	if !strings.HasPrefix(preflight, "wsl_name='sailry'\n") {
		t.Fatalf("Code Host preflight changed the installed namespace: %q", preflight)
	}
}

func TestCodeHostDoesNotUseMobileOrUnpublishedReleases(t *testing.T) {
	for _, body := range []string{
		`[]`,
		`[{"tag_name":"mobile-v0.2.14+32"}]`,
		`[{"tag_name":"v0.1.1","draft":true}]`,
		`[{"tag_name":"v0.1.1-beta.1","prerelease":true}]`,
	} {
		client := &http.Client{Transport: releaseTransport(func(request *http.Request) (*http.Response, error) {
			if request.Header.Get("Authorization") != "" {
				t.Fatal("public Host discovery sent credentials")
			}
			return &http.Response{StatusCode: http.StatusOK, Body: io.NopCloser(strings.NewReader(body))}, nil
		})}
		if version, err := resolveRelease(t.Context(), identity.CodeHost(), client); err == nil || version != "" {
			t.Errorf("unpublished Code Host selected %q from %s: %v", version, body, err)
		}
	}
}

func TestCodeHostReleaseListFollowsPages(t *testing.T) {
	first := "[" + strings.TrimSuffix(strings.Repeat(`{"tag_name":"mobile-v0.2.14+32"},`, 100), ",") + "]"
	requests := 0
	client := &http.Client{Transport: releaseTransport(func(request *http.Request) (*http.Response, error) {
		requests++
		if request.URL.Scheme != "https" || request.URL.Host != "api.github.com" || request.URL.Path != "/repos/sailry/sailry-code/releases" || request.URL.Query().Get("per_page") != "100" {
			t.Fatalf("Code pagination left its release source: %s", request.URL)
		}
		body := first
		if requests == 2 {
			if request.URL.Query().Get("page") != "2" {
				t.Fatalf("Code pagination did not advance: %s", request.URL)
			}
			body = `[{"tag_name":"v0.1.1"}]`
		} else if requests > 2 {
			t.Fatalf("Code pagination did not stop at the short page: %s", request.URL)
		}
		return &http.Response{
			StatusCode: http.StatusOK, Body: io.NopCloser(strings.NewReader(body)),
			Header: http.Header{"Link": {`<https://other.example/releases?page=2>; rel="next"`}},
		}, nil
	})}
	version, err := resolveRelease(t.Context(), identity.CodeHost(), client)
	if err != nil || version != "0.1.1" || requests != 2 {
		t.Fatalf("Code Host behind a mobile-only page = %q, %d requests: %v", version, requests, err)
	}
}

func TestCodeHostReleaseListAcceptsMultiAssetPages(t *testing.T) {
	asset := `{"name":"sailry-host.zip","browser_download_url":"https://github.com/sailry/sailry-code/releases/download/v0.1.1/sailry-host.zip","label":"` + strings.Repeat("a", 600) + `"}`
	release := `{"tag_name":"v0.1.1","assets":[` + strings.TrimSuffix(strings.Repeat(asset+",", 20), ",") + `]}`
	body := "[" + strings.TrimSuffix(strings.Repeat(release+",", 100), ",") + "]"
	if len(body) <= 1024*1024 || len(body) > 8*1024*1024 {
		t.Fatalf("multi-asset fixture has the wrong size: %d", len(body))
	}
	requests := 0
	client := &http.Client{Transport: releaseTransport(func(request *http.Request) (*http.Response, error) {
		requests++
		page := body
		if requests == 2 {
			page = "[]"
		} else if requests > 2 {
			t.Fatal("Code pagination did not stop at the empty page")
		}
		return &http.Response{StatusCode: http.StatusOK, Body: io.NopCloser(strings.NewReader(page))}, nil
	})}
	version, err := resolveRelease(t.Context(), identity.CodeHost(), client)
	if err != nil || version != "0.1.1" || requests != 2 {
		t.Fatalf("Code multi-asset release page = %q, %d requests: %v", version, requests, err)
	}
}

func TestHostReleaseMetadataLimits(t *testing.T) {
	for _, row := range []struct {
		name    string
		release identity.HostRelease
		limit   int
	}{
		{"Code release page", identity.CodeHost(), 8 * 1024 * 1024},
		{"Harness single latest", identity.OfficialHost(), 1024 * 1024},
	} {
		t.Run(row.name, func(t *testing.T) {
			body := `{"tag_name":"v0.1.1","body":"` + strings.Repeat("a", row.limit) + `"}`
			if row.release == identity.CodeHost() {
				body = "[" + body + "]"
			}
			reader := &io.LimitedReader{R: strings.NewReader(body), N: int64(len(body))}
			requests := 0
			client := &http.Client{Transport: releaseTransport(func(request *http.Request) (*http.Response, error) {
				requests++
				if row.release == identity.OfficialHost() && request.URL.String() != "https://api.github.com/repos/sailry/sailry-harness/releases/latest" {
					t.Fatalf("Harness latest source changed: %s", request.URL)
				}
				return &http.Response{StatusCode: http.StatusOK, Body: io.NopCloser(reader)}, nil
			})}
			version, err := resolveRelease(t.Context(), row.release, client)
			if err == nil || version != "" || requests != 1 {
				t.Fatalf("oversized metadata selected %q, %d requests: %v", version, requests, err)
			}
			if read := int64(len(body)) - reader.N; read > int64(row.limit+1) {
				t.Fatalf("metadata budget read %d bytes, limit %d", read, row.limit)
			}
		})
	}
}

func TestCodeHostReleaseListPageLimit(t *testing.T) {
	body := "[" + strings.TrimSuffix(strings.Repeat(`{"tag_name":"v0.1.1"},`, 100), ",") + "]"
	requests := 0
	client := &http.Client{Transport: releaseTransport(func(request *http.Request) (*http.Response, error) {
		requests++
		if page := request.URL.Query().Get("page"); requests > 1 && page != strconv.Itoa(requests) {
			t.Fatalf("Code pagination did not advance: %s", request.URL)
		}
		return &http.Response{StatusCode: http.StatusOK, Body: io.NopCloser(strings.NewReader(body))}, nil
	})}
	version, err := resolveRelease(t.Context(), identity.CodeHost(), client)
	if err == nil || version != "" || requests != 10 || !strings.Contains(err.Error(), "page limit") {
		t.Fatalf("unbounded or partial discovery = %q, %d requests: %v", version, requests, err)
	}
}

func TestHarnessHostKeepsSingleLatestDiscovery(t *testing.T) {
	requests := 0
	client := &http.Client{Transport: releaseTransport(func(request *http.Request) (*http.Response, error) {
		requests++
		if request.URL.String() != "https://api.github.com/repos/sailry/sailry-harness/releases/latest" {
			t.Fatalf("Harness latest source changed: %s", request.URL)
		}
		return &http.Response{
			StatusCode: http.StatusOK, Body: io.NopCloser(strings.NewReader(`{"tag_name":"v0.2.13"}`)),
			Header: http.Header{"Link": {`<https://other.example/releases?page=2>; rel="next"`}},
		}, nil
	})}
	if version, err := resolveRelease(t.Context(), identity.OfficialHost(), client); err != nil || version != "0.2.13" || requests != 1 {
		t.Fatalf("Harness single latest = %q, %d requests: %v", version, requests, err)
	}
}
