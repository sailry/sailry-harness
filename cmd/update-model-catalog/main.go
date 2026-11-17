// Command update-model-catalog refreshes the model metadata embedded in Sailry.
package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"slices"
	"time"

	"github.com/sailry/sailry-harness/internal/node/providers"
)

const maxCatalogBytes = 32 * 1024 * 1024

func main() {
	if err := run(context.Background(), os.Args[1:], os.Stdout); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func run(ctx context.Context, args []string, output io.Writer) error {
	flags := flag.NewFlagSet("update-model-catalog", flag.ContinueOnError)
	input := flags.String("input", "", "Read a catalog file instead of downloading it")
	destination := flags.String("output", "internal/node/providers/data/model-catalog.json", "Write the validated catalog to this file")
	if err := flags.Parse(args); err != nil {
		return err
	}
	if flags.NArg() != 0 {
		return errors.New("unexpected positional arguments")
	}
	var source io.ReadCloser
	var err error
	if *input != "" {
		source, err = os.Open(*input)
	} else {
		source, err = download(ctx, &http.Client{Timeout: 30 * time.Second}, providers.CatalogURL)
	}
	if err != nil {
		return err
	}
	defer source.Close()
	raw, err := io.ReadAll(io.LimitReader(source, maxCatalogBytes+1))
	if err != nil {
		return err
	}
	canonical, err := canonicalCatalog(raw)
	if err != nil {
		return err
	}
	changed, err := replaceCatalog(*destination, canonical)
	if err != nil {
		return err
	}
	if changed {
		fmt.Fprintln(output, "Updated bundled model catalog")
	} else {
		fmt.Fprintln(output, "Bundled model catalog is unchanged")
	}
	return nil
}

func download(ctx context.Context, client *http.Client, url string) (io.ReadCloser, error) {
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return nil, err
	}
	response, err := client.Do(request)
	if err != nil {
		return nil, err
	}
	if response.StatusCode != http.StatusOK {
		response.Body.Close()
		return nil, fmt.Errorf("model catalog download returned HTTP %d", response.StatusCode)
	}
	return response.Body, nil
}

func canonicalCatalog(raw []byte) ([]byte, error) {
	if _, err := providers.ParseCatalog(raw); err != nil {
		return nil, err
	}
	var document map[string]map[string]any
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.UseNumber()
	if err := decoder.Decode(&document); err != nil {
		return nil, err
	}
	// Keep each model on its own line so catalog changes remain reviewable.
	var result bytes.Buffer
	result.WriteString("{\n")
	providerIDs := sortedKeys(document)
	for providerIndex, providerID := range providerIDs {
		provider := document[providerID]
		name, _ := json.Marshal(providerID)
		fmt.Fprintf(&result, "  %s: {\n", name)
		fields := sortedKeys(provider)
		for fieldIndex, field := range fields {
			name, _ := json.Marshal(field)
			fmt.Fprintf(&result, "    %s: ", name)
			if field == "models" {
				models := provider[field].(map[string]any)
				result.WriteString("{\n")
				modelIDs := sortedKeys(models)
				for i, id := range modelIDs {
					name, _ := json.Marshal(id)
					body, err := json.Marshal(models[id])
					if err != nil {
						return nil, err
					}
					fmt.Fprintf(&result, "      %s: %s", name, body)
					endLine(&result, i, len(modelIDs))
				}
				result.WriteString("    }")
			} else {
				body, err := json.Marshal(provider[field])
				if err != nil {
					return nil, err
				}
				result.Write(body)
			}
			endLine(&result, fieldIndex, len(fields))
		}
		result.WriteString("  }")
		endLine(&result, providerIndex, len(providerIDs))
	}
	result.WriteString("}\n")
	if _, err := providers.ParseCatalog(result.Bytes()); err != nil {
		return nil, err
	}
	return result.Bytes(), nil
}

func sortedKeys[V any](values map[string]V) []string {
	keys := make([]string, 0, len(values))
	for key := range values {
		keys = append(keys, key)
	}
	slices.Sort(keys)
	return keys
}

func endLine(output *bytes.Buffer, index, count int) {
	if index+1 < count {
		output.WriteByte(',')
	}
	output.WriteByte('\n')
}

func replaceCatalog(destination string, raw []byte) (bool, error) {
	previous, err := os.ReadFile(destination)
	if err != nil && !errors.Is(err, os.ErrNotExist) {
		return false, err
	}
	if bytes.Equal(previous, raw) {
		return false, nil
	}
	file, err := os.CreateTemp(filepath.Dir(destination), ".model-catalog-*")
	if err != nil {
		return false, err
	}
	defer os.Remove(file.Name())
	if _, err := file.Write(raw); err != nil {
		file.Close()
		return false, err
	}
	if err := file.Chmod(0o644); err != nil {
		file.Close()
		return false, err
	}
	if err := file.Close(); err != nil {
		return false, err
	}
	if err := os.Rename(file.Name(), destination); err != nil {
		return false, err
	}
	return true, nil
}
