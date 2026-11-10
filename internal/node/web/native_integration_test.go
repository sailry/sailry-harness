//go:build integration

package web

import (
	"context"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestNativePublicFetch(t *testing.T) {
	fetcher := NewFetcher(FetchOptions{})
	defer fetcher.Close()
	page, err := fetcher.Fetch(context.Background(), protocol.WebRequest{Url: "https://example.com/"})
	if err != nil {
		t.Fatal(err)
	}
	if page.Status != 200 || page.Title == nil || *page.Title != "Example Domain" || !strings.Contains(page.Content, "documentation examples") || !page.Untrusted {
		t.Fatalf("public page = %#v", page)
	}
	if _, err := fetcher.Fetch(context.Background(), protocol.WebRequest{Url: "http://127.0.0.1/"}); err == nil {
		t.Fatal("loopback fetch reached the network")
	}
}
