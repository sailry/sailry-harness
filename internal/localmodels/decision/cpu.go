package decision

import (
	"path/filepath"
	"strings"

	"github.com/ebitengine/purego"
)

// variant reports a locked CPU plugin built for one instruction set.
func variant(name string) bool {
	return strings.HasPrefix(name, "libggml-cpu-") && strings.HasSuffix(name, ".so") ||
		strings.HasPrefix(name, "ggml-cpu-") && strings.HasSuffix(name, ".dll")
}

// selectCPU scores only locked plugins through the exported scoring ABI of
// the pinned build, as the official loader does.
func selectCPU(directory string, variants []string) (string, error) {
	name, err := best(variants, func(name string) (int32, error) {
		handle, err := library(filepath.Join(directory, name))
		if err != nil {
			return 0, err
		}
		defer closeLibrary(handle)
		symbol, err := librarySymbol(handle, "ggml_backend_score")
		if err != nil {
			return 0, &Error{Kind: Library, Message: err.Error()}
		}
		// The detector is compiled without the plugin's instruction set.
		var score func() int32
		purego.RegisterFunc(&score, symbol)
		return score(), nil
	})
	if err != nil {
		return "", err
	}
	return ordinary(filepath.Join(directory, name))
}

func best(variants []string, score func(string) (int32, error)) (string, error) {
	selected, maximum := "", int32(0)
	var failure error
	for _, name := range variants {
		value, err := score(name)
		if err != nil {
			failure = err
		} else if value > maximum {
			maximum, selected = value, name
		}
	}
	if selected != "" {
		return selected, nil
	}
	if failure != nil {
		return "", failure
	}
	return "", unavailable("no compatible CPU backend is available")
}
