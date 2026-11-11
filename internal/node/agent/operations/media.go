package operations

import (
	_ "embed"
	"encoding/json"
	"sync"
)

//go:embed media.json
var mediaSpecification []byte

// MediaSpec is one Node-owned media tool: its name, the operation that
// identifies its approval, its description, parameters and policy.
type MediaSpec struct {
	Name string `json:"name"`
	Spec
}

// MediaSpecs are the built-in media tools. No package declares or binds them.
var MediaSpecs = sync.OnceValues(func() ([]MediaSpec, error) {
	var specs []MediaSpec
	err := json.Unmarshal(mediaSpecification, &specs)
	return specs, err
})
