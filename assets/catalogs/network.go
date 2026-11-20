package catalogs

import _ "embed"

//go:embed network-presets.json
var networkPresetsJSON string

// NetworkPresetsJSON returns the bundled network preset catalog as embedded.
func NetworkPresetsJSON() string { return networkPresetsJSON }
