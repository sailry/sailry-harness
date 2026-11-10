package terminal

import (
	"maps"
	"os"
	"path/filepath"
	"runtime"
	"runtime/debug"
	"slices"
	"strings"
	"sync"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Environment returns the Node's terminal environment with the saved
// variables applied over it.
func Environment(base []string, settings protocol.TerminalSettings) []string {
	values := slices.Clone(base)
	for _, key := range slices.Sorted(maps.Keys(settings.Environment)) {
		values = append(values, key+"="+settings.Environment[key])
	}
	return values
}

// terminalVersion is the pinned MyGo release, which fixes the libghostty-vt
// build of the emulator: the plugin does not report the library's version.
var terminalVersion = sync.OnceValue(func() string {
	if build, valid := debug.ReadBuildInfo(); valid {
		for _, module := range build.Deps {
			if module.Path == "github.com/egoist/mygo" {
				return strings.TrimPrefix(module.Version, "v")
			}
		}
	}
	return ""
})

// launchVariables identify the emulator and the worktree to a local program.
// They follow every other variable, so nothing saved or inherited replaces
// them.
func launchVariables(goos, root string) []string {
	if goos == "windows" {
		root = plainWindowsPath(root)
	}
	variables := []string{"TERM_PROGRAM=ghostty", "TERM_PROGRAM_VERSION=" + terminalVersion(), "PWD=" + root}
	// The plugin starts from this process's environment and can only add to
	// it, so an inherited VTE marker is emptied where it should be absent.
	if _, inherited := os.LookupEnv("VTE_VERSION"); inherited {
		variables = append(variables, "VTE_VERSION=")
	}
	return variables
}

// shellCommand returns the program of a new shell terminal, the variables it
// adds and its working directory. A nil command starts the platform's login
// shell.
func shellCommand(goos string, settings protocol.TerminalSettings, root string) (command, variables []string, directory string, err error) {
	if settings.Shell == "" {
		return nil, nil, root, nil
	}
	command = append([]string{settings.Shell}, settings.ShellArguments...)
	if goos != "windows" {
		if !executable(settings.Shell) {
			return nil, nil, "", fault(protocol.ErrorCodeInternal, "terminal operation failed: the saved shell is not an executable file")
		}
		if len(settings.ShellArguments) == 0 {
			// An explicit program cannot carry a login argv[0], so ask for the
			// login startup files the default shell gets.
			command = append(command, "-l")
		}
		return command, []string{"SHELL=" + settings.Shell}, root, nil
	}
	separator := strings.LastIndexAny(settings.Shell, `\/`)
	if !strings.EqualFold(settings.Shell[separator+1:], "wsl.exe") {
		return command, nil, root, nil
	}
	distribution := ""
	for index := 0; index+1 < len(settings.ShellArguments); index++ {
		if settings.ShellArguments[index] == "--distribution" || settings.ShellArguments[index] == "-d" {
			distribution = settings.ShellArguments[index+1]
			break
		}
	}
	target, err := wslDirectory(root, distribution)
	if err != nil {
		return nil, nil, "", err
	}
	directory = plainWindowsPath(root)
	if strings.HasPrefix(directory, `\\`) {
		// The launcher cannot start in a UNC directory; --cd selects the worktree.
		if separator < 0 {
			return nil, nil, "", fault(protocol.ErrorCodeInvalidRequest, "WSL executable path is invalid")
		}
		directory = settings.Shell[:separator]
	}
	return append(command, "--cd", target), nil, directory, nil
}

// loginShell is the shell the terminal runtime starts for an empty command.
func loginShell(environment []string) string {
	shell := "/bin/sh"
	for _, entry := range environment {
		if value, found := strings.CutPrefix(entry, "SHELL="); found && value != "" {
			shell = value
		}
	}
	return shell
}

// plainWindowsPath removes the verbatim prefix of a Windows path.
func plainWindowsPath(path string) string {
	rest, verbatim := strings.CutPrefix(path, `\\?\`)
	if !verbatim {
		return path
	}
	if len(rest) >= 4 && strings.EqualFold(rest[:4], `UNC\`) {
		return `\\` + rest[4:]
	}
	return rest
}

// wslDirectory maps a Windows worktree root to the --cd argument of wsl.exe:
// a local drive path as it is, or the Linux path inside the selected
// distribution's own share.
func wslDirectory(root, distribution string) (string, error) {
	root = plainWindowsPath(root)
	if len(root) >= 3 && root[1] == ':' && root[2] == '\\' {
		return root, nil
	}
	if share, unc := strings.CutPrefix(root, `\\`); unc {
		server, rest, _ := strings.Cut(share, `\`)
		name, path, _ := strings.Cut(rest, `\`)
		if name != "" && (strings.EqualFold(server, "wsl$") || strings.EqualFold(server, "wsl.localhost")) {
			if !strings.EqualFold(distribution, name) {
				return "", fault(protocol.ErrorCodeInvalidRequest, "WSL worktree requires its matching distribution")
			}
			return "/" + strings.ReplaceAll(strings.TrimLeft(path, `\`), `\`, "/"), nil
		}
	}
	return "", fault(protocol.ErrorCodeInvalidRequest, "WSL worktree requires a local drive or matching distribution path")
}

func executable(path string) bool {
	info, err := os.Stat(path)
	return err == nil && info.Mode().IsRegular() && (runtime.GOOS == "windows" || info.Mode()&0111 != 0)
}

// findExecutable resolves a program name through the PATH of environment,
// with relative entries taken from root.
func findExecutable(root string, environment []string, name string) (string, bool) {
	searchPath, extensions := "", ""
	for _, entry := range environment {
		key, value, valid := strings.Cut(entry, "=")
		if valid && strings.EqualFold(key, "PATH") {
			searchPath = value
		}
		if valid && strings.EqualFold(key, "PATHEXT") {
			extensions = value
		}
	}
	suffixes := []string{""}
	if runtime.GOOS == "windows" && filepath.Ext(name) == "" {
		if extensions == "" {
			extensions = ".COM;.EXE;.BAT;.CMD"
		}
		suffixes = strings.Split(extensions, ";")
	}
	for _, directory := range filepath.SplitList(searchPath) {
		if !filepath.IsAbs(directory) {
			directory = filepath.Join(root, directory)
		}
		for _, extension := range suffixes {
			if candidate := filepath.Join(directory, name+extension); executable(candidate) {
				return candidate, true
			}
		}
	}
	return "", false
}
