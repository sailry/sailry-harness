// Package release builds signed Desktop update feeds for downstream products.
package release

import (
	"github.com/sailry/sailry-harness/internal/desktop/updater"
	"github.com/sailry/sailry-harness/sdk/product"
)

func PublishDesktop(identity product.Release, seed []byte, version, directory string, archives map[string]string) error {
	return updater.PublishRelease(identity, seed, version, directory, archives)
}
