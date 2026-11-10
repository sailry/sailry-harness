package localmodels

import (
	"errors"
	"io/fs"
	"os"
	"path/filepath"

	"github.com/sailry/sailry-harness/internal/localmodels/assets"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func folder(kind protocol.ModelsKind) string {
	switch kind {
	case protocol.ModelsKindOcr:
		return "ocr"
	case protocol.ModelsKindEmbedding:
		return "embedding"
	case protocol.ModelsKindDecision:
		return "decision"
	}
	return ""
}

// root creates the private model directories of a profile.
func root(profile string) (string, error) {
	resolved, err := filepath.EvalSymlinks(profile)
	if err == nil {
		resolved, err = filepath.Abs(resolved)
	}
	if err != nil {
		return "", Failure(err)
	}
	models := filepath.Join(resolved, "models")
	paths := []string{models}
	for _, kind := range NodeKinds {
		paths = append(paths, filepath.Join(models, folder(kind)))
	}
	for _, path := range paths {
		if err := os.Mkdir(path, 0o700); err != nil && !errors.Is(err, fs.ErrExist) {
			return "", Failure(err)
		}
		if info, err := os.Lstat(path); err != nil {
			return "", Failure(err)
		} else if !info.IsDir() {
			return "", Failure("model storage path is not a directory")
		}
	}
	return models, nil
}

// directory is where the files of one model live.
func directory(root, id string, definition *protocol.ModelsDefinition) (string, error) {
	if !ValidID(id) {
		return "", invalid("invalid local model identity")
	}
	kind := Kind(definition.Format)
	name := folder(kind)
	if name == "" {
		return "", speech()
	}
	path := filepath.Join(root, name, id)
	if kind != protocol.ModelsKindEmbedding {
		return path, nil
	}
	if Builtin(id) {
		return filepath.Join(path, definition.Source.Revision), nil
	}
	return filepath.Join(path, "files"), nil
}

func files(definition *protocol.ModelsDefinition) []assets.File {
	result := make([]assets.File, 0, len(definition.Assets))
	for _, asset := range definition.Assets {
		result = append(result, assets.File{Name: asset.Name, Size: asset.Bytes, SHA256: asset.Sha256})
	}
	return result
}

// known returns marked files whose identity is cached or recorded, without
// reading contents.
func known(path string, definition *protocol.ModelsDefinition, cached *assets.Stamp) *assets.Stamp {
	marker := ID(definition)
	found := assets.StampOf(path, files(definition), marker)
	if found != nil && (cached.Equal(found) || assets.Recorded(path, marker, found)) {
		return found
	}
	return nil
}

// unchanged compares a captured stamp with the marked files without reading
// their contents.
func unchanged(resource Resource) bool {
	found := assets.StampOf(resource.Directory, files(resource.Definition), ID(resource.Definition))
	return found != nil && found.Equal(resource.Stamp)
}

// stamp verifies files, hashing only when neither the cache nor a receipt
// covers them.
func stamp(path string, definition *protocol.ModelsDefinition, cached *assets.Stamp) *assets.Stamp {
	if found := known(path, definition, cached); found != nil {
		return found
	}
	return assets.Verified(path, files(definition), ID(definition))
}

func mark(path string, definition *protocol.ModelsDefinition, expected *assets.Stamp) (*assets.Stamp, error) {
	found, err := assets.Mark(path, files(definition), ID(definition), expected)
	if err != nil {
		return nil, Failure(err)
	}
	return found, nil
}

// remove deletes the files of one model. A missing directory is already
// removed.
func remove(root, id string, definition *protocol.ModelsDefinition) error {
	path, err := directory(root, id, definition)
	if err != nil {
		return err
	}
	parent, err := os.OpenRoot(filepath.Dir(path))
	if errors.Is(err, fs.ErrNotExist) {
		return nil
	}
	if err != nil {
		return Failure(err)
	}
	defer parent.Close()
	if err := parent.RemoveAll(filepath.Base(path)); err != nil {
		return Failure(err)
	}
	return nil
}
