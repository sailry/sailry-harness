package iroh

import (
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"

	"github.com/sailry/sailry-harness/internal/shipped"
)

// LoadDefault loads the iroh library of the running program. A release
// build passes development false and loads only the library its package
// ships, after checking it against the package seal. A development build
// may also name a library with SAILRY_IROH_LIB or use the one built into
// .runtime/native under the working directory.
func LoadDefault(development bool) error {
	if loaded.Load() != nil {
		return nil
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
	if !development {
		return loadPackaged(name)
	}
	if path := os.Getenv("SAILRY_IROH_LIB"); path != "" {
		return Load(path)
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
		// A macOS application bundle ships it in Contents/Resources.
		filepath.Join(directory, "..", "Resources", name),
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

// loadPackaged loads the library from the shipped directory only. The
// library is built with the release and has no upstream pin, so the package
// seal is its lock: without a matching record nothing is loaded.
func loadPackaged(name string) error {
	directory, err := shipped.Directory()
	if err != nil {
		return fmt.Errorf("native iroh library is unavailable: %w", err)
	}
	return loadSealed(directory, name)
}

func loadSealed(directory, name string) error {
	seal, err := shipped.Read(directory, runtime.GOOS+"/"+runtime.GOARCH)
	if errors.Is(err, fs.ErrNotExist) {
		return errors.New("native iroh library is unavailable: the package has no seal; reinstall the application")
	}
	if err != nil {
		return fmt.Errorf("native iroh library is unavailable: %w", err)
	}
	if err := seal.Check(directory, name); err != nil {
		return fmt.Errorf("native iroh library is unavailable: %w", err)
	}
	return Load(filepath.Join(directory, name))
}
