package live

import (
	"context"
	"fmt"
	"io"
	"net"
	"net/http"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/node/authorization"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestAuthorizationFixedPortCallback(t *testing.T) {
	for _, test := range []struct {
		name, path string
		manual     bool
		localhost  bool
	}{
		{"localhost with pasted fallback", "/auth/callback", true, true},
		{"another callback path with pasted fallback", "/callback", true, true},
		{"numeric loopback with pasted fallback", "/auth/callback", true, false},
		{"automatic callback", "/auth/callback", false, false},
	} {
		t.Run(test.name, func(t *testing.T) {
			reserved, err := net.Listen("tcp4", "127.0.0.1:0")
			if err != nil {
				t.Fatal(err)
			}
			port := uint16(reserved.Addr().(*net.TCPAddr).Port)
			if err := reserved.Close(); err != nil {
				t.Fatal(err)
			}
			host := "127.0.0.1"
			if test.localhost {
				host = "localhost"
			}
			expected := fmt.Sprintf("http://%s:%d%s", host, port, test.path)
			declaration := protocol.PluginModelAuthCallback{Path: test.path, Port: &port, RedirectUri: &expected, Manual: test.manual, AllowLocalhost: test.localhost}
			presentation := &protocol.PluginModelAuthPresentation{Kind: "script", Data: protocol.PluginModelAuthPresentationScriptPayload{Callback: &declaration}}
			ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
			defer cancel()
			redirect, returned, manual, err := localAuthorization(ctx, presentation, "zh-CN")
			if err != nil || redirect == nil || returned == nil || manual {
				t.Fatalf("fixed callback: redirect=%v listener=%v manual=%v error=%v", redirect, returned != nil, manual, err)
			}
			if *redirect != expected || authorization.ValidateRedirect(*redirect, declaration) != nil {
				t.Fatalf("callback does not match the provider's declared redirect: %s", *redirect)
			}
			transport := &http.Transport{}
			defer transport.CloseIdleConnections()
			browser := &http.Client{Transport: transport, Timeout: 5 * time.Second}
			response, err := browser.Get(expected + "?code=fixture-code&state=fixture-proof")
			if err != nil {
				t.Fatal(err)
			}
			body, readErr := io.ReadAll(response.Body)
			response.Body.Close()
			if response.StatusCode != http.StatusOK || readErr != nil {
				t.Fatalf("callback response: %d", response.StatusCode)
			}
			if response.Header.Get("Content-Type") != "text/html; charset=utf-8" || response.Header.Get("Cache-Control") != "no-store" ||
				!strings.Contains(string(body), i18n.In("zh-CN", "authorization_callback_return")) || strings.Contains(string(body), "fixture-code") || strings.Contains(string(body), "fixture-proof") {
				t.Fatal("callback did not return a private, localized browser landing page")
			}
			select {
			case callback := <-returned:
				fields, err := authorization.CallbackFields(callback, expected, "fixture-proof", false)
				if err != nil || fields["code"] != "fixture-code" {
					t.Fatalf("callback was not accepted by the authorization owner: %v", err)
				}
			case <-ctx.Done():
				t.Fatal(ctx.Err())
			}
		})
	}
}

func TestAuthorizationOccupiedCallbackPort(t *testing.T) {
	listener, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	port := uint16(listener.Addr().(*net.TCPAddr).Port)
	redirect := fmt.Sprintf("http://localhost:%d/auth/callback", port)
	for _, manual := range []bool{true, false} {
		t.Run(fmt.Sprintf("manual=%t", manual), func(t *testing.T) {
			declaration := protocol.PluginModelAuthCallback{Path: "/auth/callback", Port: &port, RedirectUri: &redirect, Manual: manual, AllowLocalhost: true}
			presentation := &protocol.PluginModelAuthPresentation{Kind: "script", Data: protocol.PluginModelAuthPresentationScriptPayload{Callback: &declaration}}
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			address, returned, fallback, err := localAuthorization(ctx, presentation, "en")
			if manual {
				if err != nil || address != nil || returned != nil || !fallback {
					t.Fatalf("occupied callback did not retain pasted fallback: redirect=%v listener=%v manual=%v error=%v", address, returned != nil, fallback, err)
				}
			} else if err == nil {
				t.Fatal("occupied callback without pasted support did not fail")
			}
		})
	}
}

func TestAuthorizationPasteOnlyCallback(t *testing.T) {
	redirect := "https://accounts.example.test/returned"
	declaration := protocol.PluginModelAuthCallback{Path: "/callback", RedirectUri: &redirect, Manual: true}
	presentation := &protocol.PluginModelAuthPresentation{Kind: "script", Data: protocol.PluginModelAuthPresentationScriptPayload{Callback: &declaration}}
	address, returned, manual, err := localAuthorization(t.Context(), presentation, "en")
	if err != nil || address != nil || returned != nil || !manual {
		t.Fatalf("paste-only callback changed: redirect=%v listener=%v manual=%v error=%v", address, returned != nil, manual, err)
	}
}
