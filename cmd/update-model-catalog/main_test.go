package main

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

const fixtureCatalog = `{"openai":{"name":"Fixture provider","models":{"fixture":{"id":"fixture","name":"Fixture model","reasoning":true,"reasoning_options":[{"type":"effort","values":["low","high"]}],"limit":{"context":32768,"output":4096},"cost":{"input":0.125},"future":{"integer":1234567890123456789}}}}}`

func TestBundledCatalogIsCanonical(t *testing.T) {
	raw, err := os.ReadFile(filepath.Join("..", "..", "internal", "node", "providers", "data", "model-catalog.json"))
	if err != nil {
		t.Fatal(err)
	}
	canonical, err := canonicalCatalog(raw)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(raw, canonical) {
		t.Fatal("bundled catalog differs from the updater output")
	}
}

func TestCatalogCanonicalizationPreservesMetadata(t *testing.T) {
	first, err := canonicalCatalog([]byte(fixtureCatalog))
	if err != nil {
		t.Fatal(err)
	}
	var document any
	decoder := json.NewDecoder(strings.NewReader(fixtureCatalog))
	decoder.UseNumber()
	if err := decoder.Decode(&document); err != nil {
		t.Fatal(err)
	}
	reordered, err := json.MarshalIndent(document, "", "    ")
	if err != nil {
		t.Fatal(err)
	}
	second, err := canonicalCatalog(reordered)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(first, second) {
		t.Fatal("formatting and object order changed the canonical catalog")
	}
	if !bytes.Contains(first, []byte(`"integer":1234567890123456789`)) || !bytes.Contains(first, []byte(`"input":0.125`)) || !bytes.Contains(first, []byte(`"reasoning_options":[{"type":"effort","values":["low","high"]}]`)) {
		t.Fatal("canonicalization lost model metadata or numeric precision")
	}
}

func TestInvalidCatalogPreservesPreviousFile(t *testing.T) {
	for _, input := range []string{
		`{}`,
		`{"fixture":{"models":{}}}`,
		`{"fixture":{"models":null}}`,
		`{"fixture":{"models":{"broken":{"name":"Missing identifier"}}}}`,
		`{"fixture":{"models":{"broken":{"id":"broken","reasoning_options":[{"type":"effort","values":[1]}]}}}}`,
		fixtureCatalog + `{}`,
	} {
		t.Run(input, func(t *testing.T) {
			directory := t.TempDir()
			destination := filepath.Join(directory, "catalog.json")
			source := filepath.Join(directory, "input.json")
			if err := os.WriteFile(destination, []byte(fixtureCatalog), 0o644); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(source, []byte(input), 0o644); err != nil {
				t.Fatal(err)
			}
			if err := run(t.Context(), []string{"-input", source, "-output", destination}, io.Discard); err == nil {
				t.Fatal("invalid catalog was accepted")
			}
			actual, err := os.ReadFile(destination)
			if err != nil || string(actual) != fixtureCatalog {
				t.Fatalf("previous catalog changed after validation failure: %s, %v", actual, err)
			}
		})
	}
}

func TestCatalogUpdateWritesOnlyChangedContent(t *testing.T) {
	directory := t.TempDir()
	source := filepath.Join(directory, "input.json")
	destination := filepath.Join(directory, "catalog.json")
	if err := os.WriteFile(source, []byte(fixtureCatalog), 0o644); err != nil {
		t.Fatal(err)
	}
	var output bytes.Buffer
	args := []string{"-input", source, "-output", destination}
	if err := run(t.Context(), args, &output); err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(output.String(), "Updated") {
		t.Fatalf("initial generation was not reported: %s", output.String())
	}
	oldTime := time.Unix(12345, 0)
	if err := os.Chtimes(destination, oldTime, oldTime); err != nil {
		t.Fatal(err)
	}
	output.Reset()
	if err := run(t.Context(), args, &output); err != nil {
		t.Fatal(err)
	}
	info, err := os.Stat(destination)
	if err != nil {
		t.Fatal(err)
	}
	if !info.ModTime().Equal(oldTime) || !strings.Contains(output.String(), "unchanged") {
		t.Fatalf("unchanged content was rewritten: %s, %s", info.ModTime(), output.String())
	}
	files, err := os.ReadDir(directory)
	if err != nil || len(files) != 2 {
		t.Fatalf("temporary update files were left behind: %v, %v", files, err)
	}
}

func TestCatalogSizeLimit(t *testing.T) {
	if _, err := canonicalCatalog(bytes.Repeat([]byte{' '}, maxCatalogBytes+1)); err == nil {
		t.Fatal("oversized catalog was accepted")
	}
}

func TestCatalogDownloadStatusAndCancellation(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(response http.ResponseWriter, request *http.Request) {
		if request.URL.Path == "/missing" {
			response.WriteHeader(http.StatusServiceUnavailable)
			return
		}
		io.WriteString(response, fixtureCatalog)
	}))
	defer server.Close()
	if _, err := download(t.Context(), server.Client(), server.URL+"/missing"); err == nil {
		t.Fatal("failed HTTP response was accepted")
	}
	source, err := download(t.Context(), server.Client(), server.URL)
	if err != nil {
		t.Fatal(err)
	}
	defer source.Close()
	raw, err := io.ReadAll(source)
	if err != nil || string(raw) != fixtureCatalog {
		t.Fatalf("successful HTTP response was not preserved: %s, %v", raw, err)
	}
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	if _, err := download(ctx, server.Client(), server.URL); err == nil {
		t.Fatal("cancelled download was accepted")
	}
}
