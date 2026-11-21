package model

import (
	"io"
	"net/http"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPluginCustomHeadersOverrideDefaultsWithinConfiguredOrigin(t *testing.T) {
	for _, row := range []struct {
		name, credentialHeader string
		key                    bool
	}{
		{"default credential", "Authorization", true},
		{"custom credential", "X-Model-Key", true},
		{"no credential", "Authorization", false},
		{"no custom credential", "X-Model-Key", false},
	} {
		t.Run(row.name, func(t *testing.T) {
			credentialHeader := row.credentialHeader
			credential := ""
			if row.key {
				credential = "Bearer private-token"
			}
			requests := 0
			_, binding, capture := pluginFixture(t, "", func(request *http.Request) (*http.Response, error) {
				requests++
				if request.URL.Host == "example.test" {
					if request.Header.Get(credentialHeader) != credential || request.Header.Get("X-Tenant-Id") != "tenant-a" || request.Header.Get("User-Agent") != "configured-client" || request.Header.Get("Content-Type") != "application/json" {
						t.Fatal("plugin HTTP lost configured headers or replaced its credential or framing")
					}
					count := 0
					for name := range request.Header {
						if strings.EqualFold(name, "user-agent") {
							count++
						}
					}
					if count != 1 {
						t.Fatal("case variants produced duplicate headers")
					}
				} else if request.Header.Get(credentialHeader) != "" || request.Header.Get("X-Tenant-Id") != "" || request.Header.Get("User-Agent") == "configured-client" {
					t.Fatal("configured private headers left their origin")
				}
				return &http.Response{StatusCode: http.StatusOK, Header: http.Header{}, Body: io.NopCloser(strings.NewReader(""))}, nil
			})
			if !row.key {
				binding.Secret, capture.Key = nil, nil
			}
			(**binding.Provider.Binding).KeyInjection = &protocol.PluginModelAuthInjection{Header: credentialHeader, Template: "Bearer {key}"}
			binding.Provider.Headers = map[string]string{
				"X-Tenant-ID": "tenant-a", "user-agent": "configured-client", strings.ToLower(credentialHeader): "untrusted", "Content-Type": "untrusted",
			}
			capture.Headers = http.Header{"USER-AGENT": []string{"captured-default"}}
			capture.Declaration.Origins = []string{"https://other.test"}
			language, err := NewPlugin(t.Context(), binding, capture)
			if err != nil {
				t.Fatal(err)
			}
			model := language.(*pluginModel)
			prepared := protocol.DefaultPluginModelAdapterWirePrepared()
			prepared.Headers = map[string]string{"User-Agent": "translation-default", "content-type": "application/json"}
			for _, address := range []string{"https://example.test/v1/messages", "https://other.test/messages"} {
				prepared.Url = address
				response, err := model.send(t.Context(), prepared, pluginRequestBytes)
				if err != nil {
					t.Fatal(err)
				}
				_ = response.Body.Close()
			}
			if requests != 2 || len(capture.Headers) != 1 || capture.Headers["USER-AGENT"][0] != "captured-default" {
				t.Fatal("model creation changed the captured source or skipped transport")
			}
		})
	}
}

func TestPluginPreparedAffinityHeaderCaseConflict(t *testing.T) {
	requests := 0
	model, _, _ := pluginFixture(t, "", func(request *http.Request) (*http.Response, error) {
		requests++
		if values := request.Header.Values("X-Session-Affinity"); len(values) != 1 || values[0] != "lane-a" {
			t.Errorf("affinity header = %q", values)
		}
		return &http.Response{StatusCode: http.StatusOK, Header: http.Header{}, Body: http.NoBody}, nil
	})
	prepared := protocol.DefaultPluginModelAdapterWirePrepared()
	prepared.Url = "https://example.test/v1/messages"
	prepared.Headers = map[string]string{"X-Session-Affinity": "lane-a", "x-session-affinity": "lane-a"}
	response, err := model.send(t.Context(), prepared, pluginRequestBytes)
	if err != nil {
		t.Fatal(err)
	}
	response.Body.Close()
	prepared.Headers["x-session-affinity"] = "lane-b"
	response, err = model.send(t.Context(), prepared, pluginRequestBytes)
	if response != nil {
		response.Body.Close()
	}
	if failure := Classify(err); failure == nil || failure.Kind != InvalidRequest || requests != 1 {
		t.Fatalf("conflicting affinity headers: failure=%v requests=%d", failure, requests)
	}
}
