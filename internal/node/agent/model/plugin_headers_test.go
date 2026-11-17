package model

import (
	"io"
	"net/http"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPluginCustomHeadersOverrideDefaultsWithinConfiguredOrigin(t *testing.T) {
	for _, credentialHeader := range []string{"Authorization", "X-Model-Key"} {
		t.Run(credentialHeader, func(t *testing.T) {
			requests := 0
			_, binding, capture := pluginFixture(t, "", func(request *http.Request) (*http.Response, error) {
				requests++
				if request.URL.Host == "example.test" {
					if request.Header.Get(credentialHeader) != "Bearer private-token" || request.Header.Get("X-Tenant-Id") != "tenant-a" || request.Header.Get("User-Agent") != "configured-client" || request.Header.Get("Content-Type") != "application/json" {
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
