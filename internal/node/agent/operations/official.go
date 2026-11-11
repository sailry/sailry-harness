package operations

import (
	"encoding/json"
	"io/fs"
	"sync"

	sailry "github.com/sailry/sailry-harness"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// officialTool is the public identity of one tool of a shipped package.
type officialTool struct {
	owner protocol.PluginReference
	tool  string
}

// officialTools indexes every tool the shipped packages declare, whatever
// its operation or platform, by the alias the Node registers it under.
var officialTools = sync.OnceValues(func() (map[string]officialTool, error) {
	catalog, err := Bundled()
	if err != nil {
		return nil, err
	}
	paths, err := fs.Glob(sailry.Plugins, "plugins/*/plugin.json")
	if err != nil {
		return nil, err
	}
	index := map[string]officialTool{}
	for _, name := range paths {
		data, err := fs.ReadFile(sailry.Plugins, name)
		if err != nil {
			return nil, err
		}
		var manifest struct {
			Name       string `json:"name"`
			Extensions map[string]struct {
				Tools []struct {
					Name string `json:"name"`
				} `json:"tools"`
			} `json:"extensions"`
		}
		if err = json.Unmarshal(data, &manifest); err != nil {
			return nil, err
		}
		for _, owner := range catalog.References {
			if owner.Name != manifest.Name {
				continue
			}
			for _, tool := range manifest.Extensions["dev.sailry.platform"].Tools {
				index[Alias(owner.Name, tool.Name)] = officialTool{owner: owner, tool: tool.Name}
			}
		}
	}
	return index, nil
})

// Official names a tool of a package the application ships: the package and
// the tool as the shipped manifest spells them. The owner must be the
// shipped package itself, by name and by the digest of the embedded tree,
// the same test CaptureCatalog applies. A package a user installed under a
// shipped package's name has another digest and is not official; neither is
// a shipped package updated past the embedded copy.
func Official(owner protocol.PluginReference, alias string) (pack, tool string, official bool) {
	index, err := officialTools()
	if err != nil {
		return "", "", false
	}
	entry, found := index[alias]
	if !found || entry.owner.Name != owner.Name || entry.owner.Digest != owner.Digest {
		return "", "", false
	}
	return entry.owner.Name, entry.tool, true
}

// OfficialPackages lists the names of the packages the application ships.
func OfficialPackages() ([]string, error) {
	catalog, err := Bundled()
	if err != nil {
		return nil, err
	}
	names := make([]string, len(catalog.References))
	for index, reference := range catalog.References {
		names[index] = reference.Name
	}
	return names, nil
}
