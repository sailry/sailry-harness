//go:build integration

package node

import (
	"context"
	"encoding/json"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/localmodels"
	"github.com/sailry/sailry-harness/internal/localmodels/assets"
	"github.com/sailry/sailry-harness/internal/localmodels/onnx"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// The knowledge test indexes a small folder with the pinned embedding and OCR
// models on the pinned ONNX Runtime:
//
//	SAILRY_ONNXRUNTIME_DIR      pinned ONNX Runtime library
//	SAILRY_EMBEDDING_MODEL_DIR  granite-multi: config.json, tokenizer.json, model.onnx
//	SAILRY_OCR_MODEL_DIR        PP-OCRv6 tiny files under their asset names
//	SAILRY_LOCALMODELS_FIXTURES ocr/english.png, ocr/scan.pdf and rust-ocr.json

// knowledgeWait bounds an indexing pass: it loads a 390 MB embedding model
// and recognizes one image and one scanned document.
const knowledgeWait = 180 * time.Second

func knowledgeEnvironment(t *testing.T, name string) string {
	t.Helper()
	value := os.Getenv(name)
	if value == "" {
		t.Fatalf("%s must be set", name)
	}
	return value
}

// knowledgeInstall places the files of a pinned model where an installation
// leaves them. A large file is linked when the profile shares its volume.
func knowledgeInstall(t *testing.T, definition *protocol.ModelsDefinition, source, installed string) {
	t.Helper()
	if err := os.MkdirAll(installed, 0o700); err != nil {
		t.Fatal(err)
	}
	for _, asset := range definition.Assets {
		from, to := filepath.Join(source, asset.Name), filepath.Join(installed, asset.Name)
		if os.Link(from, to) == nil {
			continue
		}
		input, err := os.Open(from)
		if err != nil {
			t.Fatal(err)
		}
		output, err := os.OpenFile(to, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o600)
		if err == nil {
			_, err = io.Copy(output, input)
			if closed := output.Close(); err == nil {
				err = closed
			}
		}
		input.Close()
		if err != nil {
			t.Fatal(err)
		}
	}
	if err := assets.WriteMarker(installed, localmodels.ID(definition)); err != nil {
		t.Fatal(err)
	}
}

func knowledgeCall[T any](t *testing.T, node *Node, command protocol.Command) T {
	t.Helper()
	accepted, err := node.Dispatch(t.Context(), node.ID(), protocol.NewRequest(node.ID(), command))
	if err != nil {
		t.Fatalf("%s was not admitted: %v", command.Kind, err)
	}
	var result T
	select {
	case response, delivered := <-accepted.Completion:
		if !delivered || response.Fault != nil {
			t.Fatalf("%s: delivered %v, fault %+v", command.Kind, delivered, response.Fault)
		}
		// A durable replay returns decoded JSON, so every answer is read as JSON.
		body, err := json.Marshal(response.Output.Data)
		if err == nil {
			err = json.Unmarshal(body, &result)
		}
		if err != nil {
			t.Fatalf("%s output %T: %v", command.Kind, response.Output.Data, err)
		}
	case <-time.After(knowledgeWait):
		t.Fatalf("%s did not complete within %s", command.Kind, knowledgeWait)
	}
	return result
}

// knowledgeReady waits until the collection finished a pass that satisfies
// done and returns it.
func knowledgeReady(t *testing.T, node *Node, id protocol.KnowledgeID, done func(protocol.KnowledgeCollection) bool) protocol.KnowledgeCollection {
	t.Helper()
	deadline := time.Now().Add(knowledgeWait)
	for {
		for _, collection := range knowledgeCall[[]protocol.KnowledgeCollection](t, node, protocol.Command{Kind: "list_knowledge_collections"}) {
			if collection.ID == id && collection.Progress.Status == protocol.KnowledgeStatusReady && done(collection) {
				return collection
			}
			if collection.ID == id && time.Now().After(deadline) {
				t.Fatalf("collection did not finish indexing: %+v", collection)
			}
		}
		if time.Now().After(deadline) {
			t.Fatal("collection is not listed")
		}
		time.Sleep(100 * time.Millisecond)
	}
}

// TestKnowledgeIndexing indexes text, an image and a scanned document through
// the Node, searches them by meaning and by recognized words, reads a
// citation, and re-indexes for keywords once the embedding model is turned off.
func TestKnowledgeIndexing(t *testing.T) {
	library := knowledgeEnvironment(t, "SAILRY_ONNXRUNTIME_DIR")
	embeddingFiles := knowledgeEnvironment(t, "SAILRY_EMBEDDING_MODEL_DIR")
	ocrFiles := knowledgeEnvironment(t, "SAILRY_OCR_MODEL_DIR")
	fixtures := knowledgeEnvironment(t, "SAILRY_LOCALMODELS_FIXTURES")
	if err := onnx.Load(library); err != nil {
		t.Fatal(err)
	}

	folder := filepath.Join(t.TempDir(), "notes")
	if err := os.MkdirAll(filepath.Join(folder, "scans"), 0o700); err != nil {
		t.Fatal(err)
	}
	texts := map[string]string{
		"coffee.md":  "# Espresso machine\n\nRun a descaling cycle every three months: fill the tank with citric acid solution and flush twice with clean water.\n",
		"bicycle.md": "# Bicycle\n\nInflate the tyres to 6 bar and oil the chain after riding in the rain.\n",
		"花园.md":      "# 花园\n\n番茄需要每天浇水，并且在六月搭好支架。\n",
	}
	for name, text := range texts {
		if err := os.WriteFile(filepath.Join(folder, name), []byte(text), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	for _, name := range []string{"english.png", "scan.pdf"} {
		data, err := os.ReadFile(filepath.Join(fixtures, "ocr", name))
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(folder, "scans", name), data, 0o600); err != nil {
			t.Fatal(err)
		}
	}

	profile := filepath.Join(t.TempDir(), "profile")
	recognition := localmodels.OCR()[0]
	knowledgeInstall(t, &recognition, ocrFiles, filepath.Join(profile, "models", "ocr", localmodels.ID(&recognition)))
	// The packaged library location is not where this test loads it from.
	capabilities := LocalModelCapabilities()
	for _, kind := range []protocol.ModelsKind{protocol.ModelsKindEmbedding, protocol.ModelsKindOcr} {
		capability := capabilities[kind]
		capability.Available = nil
		capabilities[kind] = capability
	}
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	node, err := StartWithOptions(t.Context(), profile, Options{
		Environment: &environment, MCP: fixtureMCPOptions(),
		LocalModels: localmodels.Options{Capabilities: capabilities},
	})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { node.Stop(context.Background()) })
	model, err := node.localModels.registry.Definition(localmodels.DefaultEmbedding)
	if err != nil {
		t.Fatal(err)
	}
	knowledgeInstall(t, model, embeddingFiles, filepath.Join(profile, "models", "embedding", localmodels.DefaultEmbedding, model.Source.Revision))

	created := knowledgeCall[protocol.KnowledgeCollection](t, node, protocol.Command{Kind: "create_knowledge_collection", Data: &protocol.CommandCreateKnowledgeCollectionPayload{
		Definition: protocol.KnowledgeDefinition{Name: "Notes", Globs: []string{}, Tags: []string{}, SharedWith: []string{},
			Source: protocol.KnowledgeSource{Kind: "directory", Data: &protocol.KnowledgeSourceDirectoryPayload{Path: folder}}},
	}})
	indexed := knowledgeReady(t, node, created.ID, func(protocol.KnowledgeCollection) bool { return true })
	progress := indexed.Progress
	if progress.Model == nil || *progress.Model != localmodels.DefaultEmbedding || progress.Sources != 5 || progress.Completed != 5 || progress.Skipped != 0 || progress.Chunks < 5 {
		t.Fatalf("indexed progress %+v", progress)
	}
	t.Logf("indexed %d sources into %d chunks with %s", progress.Sources, progress.Chunks, *progress.Model)

	search := func(mode protocol.KnowledgeMode, text string) protocol.KnowledgePage {
		t.Helper()
		return knowledgeCall[protocol.KnowledgePage](t, node, protocol.Command{Kind: "query_knowledge", Data: &protocol.KnowledgeQuery{
			Query: text, Collections: []protocol.KnowledgeID{}, Mode: mode, Tags: []string{}, Limit: 5,
		}})
	}
	top := func(page protocol.KnowledgePage, path string) protocol.KnowledgeHit {
		t.Helper()
		// A directory source cites the controller's own path.
		if len(page.Hits) == 0 || page.Hits[0].Citation.Path != filepath.Join(folder, path) || !page.Untrusted {
			t.Fatalf("top hit is not %s: %+v", path, page)
		}
		return page.Hits[0]
	}
	// Neither question shares a word with the note that answers it.
	for question, path := range map[string]string{
		"how do I remove limescale from my coffee maker": "coffee.md",
		"watering vegetables in the summer":              "花园.md",
	} {
		page := search(protocol.KnowledgeModeSemantic, question)
		if page.Mode != protocol.KnowledgeModeSemantic || page.Model == nil || *page.Model != localmodels.DefaultEmbedding {
			t.Fatalf("%q searched as %+v", question, page)
		}
		top(page, path)
	}
	// Recognized words are searchable by keyword and by both rankings.
	top(search(protocol.KnowledgeModeKeyword, "Invoice"), "scans/english.png")
	top(search(protocol.KnowledgeModeHybrid, "Sailry local models spike invoice total"), "scans/english.png")
	scanned := search(protocol.KnowledgeModeKeyword, "scan")
	found := false
	for _, hit := range search(protocol.KnowledgeModeHybrid, "scanned page text").Hits {
		found = found || hit.Citation.Path == filepath.Join(folder, "scans", "scan.pdf")
	}
	if !found {
		t.Fatalf("the scanned document has no hit: keyword %+v", scanned)
	}

	hit := top(search(protocol.KnowledgeModeKeyword, "descaling"), "coffee.md")
	read := knowledgeCall[protocol.KnowledgeCitationPage](t, node, protocol.Command{Kind: "read_knowledge_citation", Data: &protocol.KnowledgeCitationRequest{
		Collection: hit.Citation.Collection, Chunk: hit.Citation.Chunk, Revision: hit.Citation.Revision, Context: true, Limit: 16 * 1024,
	}})
	if read.Text != texts["coffee.md"] || read.NextOffset != nil || !read.Untrusted || read.Citation.Link != hit.Citation.Link {
		t.Fatalf("citation %+v", read)
	}

	// Turning the embedding model off clears the vectors and keeps keywords.
	settings := knowledgeCall[protocol.ModelsLocalSettings](t, node, protocol.Command{Kind: "read_local_model_settings"})
	settings.Choices[protocol.ModelsKindEmbedding] = protocol.ModelsLocalChoice{Enabled: false}
	knowledgeCall[protocol.ModelsLocalSettings](t, node, protocol.Command{Kind: "save_local_model_settings", Data: settings})
	keywords := knowledgeReady(t, node, created.ID, func(collection protocol.KnowledgeCollection) bool {
		return collection.Progress.Generation > progress.Generation
	})
	if keywords.Progress.Model != nil || keywords.Progress.Sources != 5 || keywords.Progress.Chunks != progress.Chunks {
		t.Fatalf("keyword progress %+v", keywords.Progress)
	}
	fallback := search(protocol.KnowledgeModeSemantic, "descaling cycle")
	if fallback.Mode != protocol.KnowledgeModeKeyword || fallback.Model != nil {
		t.Fatalf("search without a model ran as %+v", fallback)
	}
	if text := top(fallback, "coffee.md").Text; !strings.Contains(text, "descaling") {
		t.Fatalf("keyword hit text %q", text)
	}
}
