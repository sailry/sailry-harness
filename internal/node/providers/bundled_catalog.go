package providers

import (
	_ "embed"
	"strings"
	"sync"
)

//go:embed data/model-catalog.json
var bundledCatalogJSON []byte

var bundledCatalog = sync.OnceValues(func() (*CatalogDownload, error) {
	return ParseCatalog(bundledCatalogJSON)
})

// BundledCatalog provides the build's validated offline metadata snapshot.
// Its returned entries and bodies are shared and must not be modified.
func BundledCatalog() (*CatalogDownload, error) {
	return bundledCatalog()
}

// BundledCatalogProvider resolves an unambiguous catalog endpoint from the
// pinned snapshot independently of which model metadata snapshot is active.
func BundledCatalogProvider(endpoint string) (string, error) {
	catalog, err := BundledCatalog()
	if err != nil {
		return "", err
	}
	return catalog.endpoints[strings.TrimRight(endpoint, "/")], nil
}
