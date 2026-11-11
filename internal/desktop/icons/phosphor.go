package icons

import (
	"archive/zip"
	"bytes"
	_ "embed"
	"fmt"
	"io"
	"strings"
	"sync"

	"github.com/egoist/mygo/ui"
)

// Phosphor Icons, fill weight, are the glyphs plugins declare as
// "phosphor:<name>": one path each on a 256 unit box, drawn in the text
// color. The whole set is embedded as one archive (phosphor/SOURCE), read
// when an icon is first asked for.
//
//go:embed phosphor/fill.zip
var phosphorArchive []byte

// phosphorFiles are the archive's entries by icon name.
var phosphorFiles = sync.OnceValue(func() map[string]*zip.File {
	archive, err := zip.NewReader(bytes.NewReader(phosphorArchive), int64(len(phosphorArchive)))
	if err != nil {
		panic(err)
	}
	byName := make(map[string]*zip.File, len(archive.File))
	for _, file := range archive.File {
		byName[strings.TrimSuffix(file.Name, ".svg")] = file
	}
	return byName
})

// phosphorIcons are the icons parsed so far, by name.
var phosphorIcons sync.Map

// phosphor is the icon the set calls name.
func phosphor(name string) (*ui.SVG, error) {
	if cached, exists := phosphorIcons.Load(name); exists {
		return cached.(*ui.SVG), nil
	}
	file := phosphorFiles()[name]
	if file == nil {
		return nil, fmt.Errorf("plugin icon %q is unavailable", "phosphor:"+name)
	}
	reader, err := file.Open()
	if err != nil {
		return nil, fmt.Errorf("read plugin icon %q: %w", "phosphor:"+name, err)
	}
	data, err := io.ReadAll(reader)
	reader.Close()
	if err != nil {
		return nil, fmt.Errorf("read plugin icon %q: %w", "phosphor:"+name, err)
	}
	icon, err := ui.ParseSVG(data)
	if err != nil {
		return nil, fmt.Errorf("parse plugin icon %q: %w", "phosphor:"+name, err)
	}
	cached, _ := phosphorIcons.LoadOrStore(name, icon)
	return cached.(*ui.SVG), nil
}
