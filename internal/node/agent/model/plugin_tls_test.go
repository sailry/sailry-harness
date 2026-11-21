package model

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"errors"
	"net/http"
	"net/http/httptest"
	"net/http/httptrace"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

func TestPluginCertificateVerificationDoesNotRetry(t *testing.T) {
	server := httptest.NewTLSServer(http.HandlerFunc(func(http.ResponseWriter, *http.Request) {
		t.Error("model request sent without a trusted certificate")
	}))
	t.Cleanup(server.Close)
	base := server.Client().Transport.(*http.Transport).Clone()
	base.TLSClientConfig.RootCAs = x509.NewCertPool()
	t.Cleanup(base.CloseIdleConnections)
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	var handshakes atomic.Int32
	ctx = httptrace.WithClientTrace(ctx, &httptrace.ClientTrace{TLSHandshakeDone: func(_ tls.ConnectionState, err error) {
		if _, ok := errors.AsType[*tls.CertificateVerificationError](err); !ok {
			t.Errorf("TLS handshake did not report a verification error: %v", err)
		}
		handshakes.Add(1)
	}})
	_, binding, capture := pluginFixture(t, `export function prepare(input) { return {method:"POST",url:input.endpoint,body:"{}"}; }`, nil)
	binding.Provider.Endpoint = server.URL
	capture.Client = &http.Client{Transport: base}
	language, err := NewPlugin(ctx, binding, capture)
	if err != nil {
		t.Fatal(err)
	}
	call, err := BuildCall(binding, Request{System: "fixture"})
	if err != nil {
		t.Fatal(err)
	}
	timer := &policyClock{}
	var attempts []Attempt
	err = Run(ctx, language, call, Main, timer, func(attempt Attempt) error {
		attempts = append(attempts, attempt)
		return nil
	}, nil, nil)
	failure := Classify(err)
	if failure == nil || failure.Kind != InvalidRequest || failure.Retryable() || !strings.HasPrefix(failure.Message, "model connection failed: tls: failed to verify certificate") {
		t.Fatalf("unexpected certificate verification failure: %+v", failure)
	}
	if len(attempts) != 1 || attempts[0].RetryDelay != nil || attempts[0].Status != nil || attempts[0].ResponseUs != nil || len(timer.delays) != 0 || handshakes.Load() != 1 {
		t.Fatalf("certificate verification recovery: attempts=%+v waits=%v handshakes=%d", attempts, timer.delays, handshakes.Load())
	}
}

func TestPluginDiscoveryCertificateDiagnosticRedactsCredentials(t *testing.T) {
	const credential = "private-token"
	// Put the credential across the diagnostic length limit.
	const prefix = "tls: failed to verify certificate: "
	padding := strings.Repeat("x", 200-len(prefix)-1-len("[redacted]"))
	cause := &tls.CertificateVerificationError{Err: errors.New(padding + " " + credential)}
	_, binding, capture := pluginFixture(t, `export function prepare_discovery(input) { return {method:"GET",url:input.endpoint}; }`, func(request *http.Request) (*http.Response, error) {
		if request.Body != nil {
			request.Body.Close()
		}
		return nil, cause
	})
	capture.Declaration.Discovery = true
	_, err := DiscoverPlugin(t.Context(), binding, capture)
	failure, ok := errors.AsType[*Failure](err)
	if !ok || failure.Kind != InvalidRequest || !strings.Contains(failure.Message, "[redacted]") || strings.Contains(failure.Message, "private-") {
		t.Fatalf("unexpected discovery diagnostic: %+v", err)
	}
}
