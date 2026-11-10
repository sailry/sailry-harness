package python

import (
	"context"
	_ "embed"
	"errors"
	"os"
	"path/filepath"
	"runtime"

	"github.com/sailry/sailry-harness/internal/protocol"
)

//go:embed relocate.py
var relocateScript string

func (manager *Manager) command(python string, arguments []string, directory string) Command {
	environment := make(map[string]string)
	for _, entry := range manager.options.Environment {
		for index, character := range entry {
			if character == '=' {
				environment[entry[:index]] = entry[index+1:]
				break
			}
		}
	}
	devNull := "/dev/null"
	if runtime.GOOS == "windows" {
		devNull = "NUL"
	}
	environment["PIP_CONFIG_FILE"] = devNull
	environment["PIP_CACHE_DIR"] = filepath.Join(manager.root, "cache")
	environment["PYTHONDONTWRITEBYTECODE"] = "1"
	environment["PYTHONNOUSERSITE"] = "1"
	return Command{Executable: python, Arguments: append([]string{"-B"}, arguments...), Directory: directory, Environment: environment}
}

func (manager *Manager) prepare(ctx context.Context, requirements *Requirements, runtime *Runtime, destination, key string) error {
	manager.report(key, protocol.PythonStageCreating)
	stage, err := os.MkdirTemp(filepath.Join(manager.root, "staging"), "environment-")
	if err != nil {
		return storage(err)
	}
	defer os.RemoveAll(stage)
	run := func(executable string, arguments []string) error {
		if err := check(ctx); err != nil {
			return err
		}
		return manager.options.Run(ctx, manager.command(executable, arguments, stage))
	}
	if err := run(runtime.Python, []string{"-I", "-m", "venv", "--copies", stage}); err != nil {
		return err
	}
	python := environmentInterpreter(stage)
	requirementsPath := filepath.Join(stage, ".sailry-requirements.txt")
	if err := writePrivate(requirementsPath, []byte(requirements.Content)); err != nil {
		return err
	}
	manager.report(key, protocol.PythonStageInstalling)
	arguments := []string{"-I", "-m", "pip", "--isolated", "install", "--disable-pip-version-check", "--no-input", "--require-hashes", "--only-binary=:all:", "--no-deps", "--no-compile", "--cache-dir", filepath.Join(manager.root, "cache"), "-r", requirementsPath}
	manager.mu.Lock()
	index := copyString(manager.index)
	manager.mu.Unlock()
	if index != nil {
		arguments = append(arguments, "--index-url", *index)
	}
	if err := run(python, arguments); err != nil {
		return err
	}
	manager.report(key, protocol.PythonStageValidating)
	if err := run(python, []string{"-I", "-m", "pip", "--isolated", "check", "--disable-pip-version-check"}); err != nil {
		return err
	}
	if err := run(python, []string{"-I", "-c", relocateScript, destination}); err != nil {
		return err
	}
	if err := seal(ctx, stage, runtime.Root, key); err != nil {
		return err
	}
	if err := check(ctx); err != nil {
		return err
	}
	if info, err := os.Lstat(destination); err == nil {
		if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
			return invalid("Python environment destination is not a directory")
		}
		if err := os.RemoveAll(destination); err != nil {
			return storage(err)
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return storage(err)
	}
	if err := os.Rename(stage, destination); err != nil {
		return storage(err)
	}
	return check(ctx)
}
