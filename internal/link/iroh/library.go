package iroh

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"
)

func LoadDefault() error {
	if loaded.Load() != nil {
		return nil
	}
	if path := os.Getenv("SAILRY_IROH_LIB"); path != "" {
		return Load(path)
	}
	extension := ".so"
	if runtime.GOOS == "darwin" || runtime.GOOS == "ios" {
		extension = ".dylib"
	}
	if runtime.GOOS == "windows" {
		extension = ".dll"
	}
	name := "libsailry_iroh" + extension
	if runtime.GOOS == "windows" {
		name = "sailry_iroh.dll"
	}
	if runtime.GOOS == "android" {
		return Load(name)
	}
	executable, err := os.Executable()
	if err != nil {
		return err
	}
	directory := filepath.Dir(executable)
	candidates := []string{
		filepath.Join(directory, name),
		filepath.Join(directory, "native", name),
		filepath.Join(directory, "Frameworks", "SailryIroh.framework", "SailryIroh"),
		filepath.Join(directory, "..", "Frameworks", "SailryIroh.framework", "SailryIroh"),
		filepath.Join(".runtime", "native", runtime.GOOS+"-"+runtime.GOARCH, name),
	}
	for _, path := range candidates {
		if _, err := os.Stat(path); err == nil {
			return Load(path)
		}
	}
	return fmt.Errorf("native iroh library is unavailable; set SAILRY_IROH_LIB or bundle %s", name)
}
