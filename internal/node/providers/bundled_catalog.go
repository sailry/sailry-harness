package providers

import (
	_ "embed"
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
