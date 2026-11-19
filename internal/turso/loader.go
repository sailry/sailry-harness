package turso

import (
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"strings"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/shipped"
	turso_libs "github.com/tursodatabase/turso-go-platform-libs"
	sdk "turso.tech/database/tursogo"
)

// engineLibrary is the same verified runtime initialized by the official SDK.
// The local bridge uses only its open, close and interrupt extension symbols.
var engineLibrary uintptr

func initializeLibrary() (err error) {
	name, variable := libraryPlatform()
	if name == "" {
		return fmt.Errorf("Turso is unavailable on %s", runtime.GOOS)
	}
	directory, err := libraryDirectory(name, !core.Release())
	if err != nil {
		return err
	}
	// The official SDK accepts a search strategy, but no library path. Restrict
	// its search to the selected directory during its one-time initialization;
	// restore the process environment before any database connection is opened.
	// In particular, never fall back to the SDK's embedded non-FTS library.
	previous, present := os.LookupEnv(variable)
	if err := os.Setenv(variable, directory); err != nil {
		return err
	}
	defer func() {
		var restore error
		if present {
			restore = os.Setenv(variable, previous)
		} else {
			restore = os.Unsetenv(variable)
		}
		err = errors.Join(err, restore)
	}()
	engineLibrary, err = turso_libs.LoadTursoLibrary(turso_libs.LoadTursoLibraryConfig{LoadStrategy: turso_libs.SystemLibraryLoadStrategy})
	if err != nil {
		return err
	}
	sdk.InitLibrary(turso_libs.LoadTursoLibraryConfig{LoadStrategy: turso_libs.SystemLibraryLoadStrategy})
	return nil
}

func libraryPlatform() (name, variable string) {
	switch runtime.GOOS {
	case "darwin":
		return "libturso_sync_sdk_kit.dylib", "DYLD_LIBRARY_PATH"
	case "linux":
		return "libturso_sync_sdk_kit.so", "LD_LIBRARY_PATH"
	case "windows":
		return "turso_sync_sdk_kit.dll", "PATH"
	}
	return "", ""
}

func libraryDirectory(name string, development bool) (string, error) {
	if development {
		if path := os.Getenv("SAILRY_TURSO_LIB"); path != "" {
			if filepath.Base(path) != name {
				return "", fmt.Errorf("SAILRY_TURSO_LIB must name %s", name)
			}
			return verifyLibrary(filepath.Dir(path), name, false)
		}
	}
	directory, err := shipped.Directory()
	if err != nil {
		return "", err
	}
	if !development {
		return verifyLibrary(directory, name, true)
	}
	if _, err := os.Lstat(filepath.Join(directory, name)); !errors.Is(err, fs.ErrNotExist) {
		return verifyLibrary(directory, name, false)
	}
	// Tests and go run use the working checkout. A source binary built in the
	// checkout can also be launched from a different working directory.
	cwd, _ := os.Getwd()
	if staged := shipped.StagedDirectory(name, cwd, directory); staged != "" {
		return verifyLibrary(staged, name, false)
	}
	return "", fmt.Errorf("Turso native library is unavailable; run sh native/turso/build.sh or set SAILRY_TURSO_LIB to %s", name)
}

func verifyLibrary(directory, name string, requireSeal bool) (string, error) {
	directory, err := filepath.Abs(directory)
	if err == nil {
		directory, err = filepath.EvalSymlinks(directory)
	}
	if err != nil {
		return "", err
	}
	// The SDK splits this environment variable into directories. Reject an
	// ambiguous directory so it cannot select another file before ours.
	if strings.ContainsRune(directory, os.PathListSeparator) {
		return "", errors.New("Turso library directory contains a path-list separator")
	}
	seal, err := shipped.Read(directory, runtime.GOOS+"/"+runtime.GOARCH)
	if errors.Is(err, fs.ErrNotExist) && !requireSeal {
		_, err = shipped.Record(directory, name)
	} else if errors.Is(err, fs.ErrNotExist) {
		err = errors.New("the package has no native seal; reinstall the application")
	} else if err == nil {
		err = seal.Check(directory, name)
		if err == nil {
			err = seal.CheckWindowsCRT(directory)
		}
	}
	if err != nil {
		return "", fmt.Errorf("Turso native library is unavailable: %w", err)
	}
	return directory, nil
}
