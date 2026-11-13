package assets

import (
	"context"
	"crypto/sha256"
	"errors"
	"fmt"
	"net/http"
	"net/url"
	"strings"
	"testing"
)

type failingTransport func(*http.Request) (*http.Response, error)

func (transport failingTransport) RoundTrip(request *http.Request) (*http.Response, error) {
	return transport(request)
}

func TestDownloadFailure(t *testing.T) {
	for _, cancelled := range []bool{false, true} {
		t.Run(fmt.Sprintf("cancelled=%t", cancelled), func(t *testing.T) {
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			client := Client(failingTransport(func(request *http.Request) (*http.Response, error) {
				if cancelled {
					cancel()
					return nil, request.Context().Err()
				}
				return nil, errors.New("fixture connection refused")
			}))
			download := Download{
				File: File{Name: "model.bin", Size: 1, SHA256: fmt.Sprintf("%x", sha256.Sum256([]byte{1}))},
				URL:  &url.URL{Scheme: "https", Host: "models.invalid", Path: "/model.bin", RawQuery: "token=private"},
			}
			err := Prepare(ctx, client, t.TempDir(), []Download{download}, func(Progress) {})
			if cancelled {
				if !errors.Is(err, ErrCancelled) {
					t.Fatalf("cancelled download: %v", err)
				}
			} else if !errors.Is(err, ErrDownload) || !strings.Contains(err.Error(), "fixture connection refused") {
				t.Fatalf("transport failure lost its cause: %v", err)
			}
			if strings.Contains(err.Error(), "private") || strings.Contains(err.Error(), "models.invalid") {
				t.Fatalf("download failure exposed its URL: %v", err)
			}
		})
	}
}
