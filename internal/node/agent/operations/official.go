package operations

import "github.com/sailry/sailry-harness/internal/protocol"

// Official names an exact tool declaration owned by the bundled inventory.
// Installed package names and origins do not establish this content identity.
func Official(owner protocol.PluginReference, alias string) (pack, tool string, official bool) {
	catalog, err := Bundled()
	if err != nil {
		return "", "", false
	}
	for _, declaration := range catalog.Tools {
		if declaration.Name == alias && declaration.Owner.Name == owner.Name && declaration.Owner.Digest == owner.Digest {
			return owner.Name, declaration.Exposed, true
		}
	}
	return "", "", false
}

// OfficialPackages lists the names in the bundled inventory, not installed names.
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
