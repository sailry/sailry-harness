package document

import (
	"encoding/json"
	"fmt"
	"io/fs"

	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

// The digest encoding is independent of wire types, tool metadata and the
// legacy package archive digest. Preserve its field and list order. A new
// renderer can use this encoding with its own version and immutable resources.
const digestDomainV1 = "sailry.native/brief/document/identity/v1\x00"

type resourceDigestV1 struct {
	Path   string `json:"path"`
	Digest string `json:"digest"`
}

func digestV1(version string, view protocol.ArtifactView, tree fs.FS) (string, error) {
	read := func(path string) (resourceDigestV1, error) {
		body, err := fs.ReadFile(tree, path)
		if err != nil {
			return resourceDigestV1{}, fmt.Errorf("Brief renderer %s resource %q: %w", version, path, err)
		}
		return resourceDigestV1{Path: path, Digest: fmt.Sprintf("%x", blake3.Sum256(body))}, nil
	}
	readList := func(paths []string) ([]resourceDigestV1, error) {
		resources := make([]resourceDigestV1, 0, len(paths))
		for _, path := range paths {
			resource, err := read(path)
			if err != nil {
				return nil, err
			}
			resources = append(resources, resource)
		}
		return resources, nil
	}

	template, err := read(view.Template)
	if err != nil {
		return "", err
	}
	styles, err := readList(view.Styles)
	if err != nil {
		return "", err
	}
	scripts, err := readList(view.Scripts)
	if err != nil {
		return "", err
	}
	// The controller owns the offline binaries; the renderer owns their names
	// and load order. Encode only the declaration here.
	libraries := make([]string, len(view.Libraries))
	for i, library := range view.Libraries {
		libraries[i] = string(library)
	}
	identity := struct {
		Version        string             `json:"version"`
		Template       resourceDigestV1   `json:"template"`
		Styles         []resourceDigestV1 `json:"styles"`
		Scripts        []resourceDigestV1 `json:"scripts"`
		Libraries      []string           `json:"libraries"`
		AllowedClasses []string           `json:"allowed_classes"`
	}{version, template, styles, scripts, libraries, append([]string{}, view.AllowedClasses...)}
	encoded, err := json.Marshal(identity)
	if err != nil {
		return "", err
	}
	return fmt.Sprintf("%x", blake3.Sum256(append([]byte(digestDomainV1), encoded...))), nil
}
