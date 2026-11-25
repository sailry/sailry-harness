// Package i18n shares window-local translation catalogs with native pages.
package i18n

import (
	"io/fs"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
)

type Catalog = i18n.Catalog

func New(sources ...fs.FS) *Catalog                   { return i18n.New(sources...) }
func Bind(c *ui.Context, catalog *Catalog)            { i18n.Bind(c, catalog) }
func T(c *ui.Context, key string, args ...any) string { return i18n.T(c, key, args...) }
func In(language, key string, args ...any) string     { return i18n.In(language, key, args...) }
