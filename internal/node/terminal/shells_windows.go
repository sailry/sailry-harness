package terminal

import (
	"os"
	"path/filepath"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/sys/windows/registry"
)

func shellCandidates(environment []string) []string {
	paths := []string{}
	if system := os.Getenv("SystemRoot"); system != "" {
		paths = append(paths,
			filepath.Join(system, "System32", "cmd.exe"),
			filepath.Join(system, "System32", "WindowsPowerShell", "v1.0", "powershell.exe"),
			filepath.Join(system, "SysWOW64", "WindowsPowerShell", "v1.0", "powershell.exe"))
	}
	for _, name := range []string{"ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"} {
		if root := os.Getenv(name); root != "" {
			paths = append(paths, powershellVersions(filepath.Join(root, "PowerShell"))...)
			paths = append(paths, gitShell(filepath.Join(root, "Git"))...)
			paths = append(paths, additionalShells(root)...)
		}
	}
	if local := os.Getenv("LOCALAPPDATA"); local != "" {
		paths = append(paths, powershellVersions(filepath.Join(local, "Microsoft", "PowerShell"))...)
		paths = append(paths, powershellVersions(filepath.Join(local, "Programs", "PowerShell"))...)
		paths = append(paths, filepath.Join(local, "Microsoft", "WinGet", "Links", "pwsh.exe"))
		paths = append(paths, gitShell(filepath.Join(local, "Programs", "Git"))...)
		paths = append(paths, additionalShells(filepath.Join(local, "Programs"))...)
		paths = append(paths, filepath.Join(local, "Microsoft", "WinGet", "Links", "nu.exe"))
	}
	if drive := os.Getenv("SystemDrive"); drive != "" {
		for _, name := range []string{"msys64", "msys32", "msys2", "cygwin64", "cygwin"} {
			paths = append(paths, filepath.Join(drive+`\`, name, "usr", "bin", "bash.exe"), filepath.Join(drive+`\`, name, "bin", "bash.exe"))
		}
	}
	if shell := environmentValue(environment, "COMSPEC"); shell != "" {
		paths = append(paths, shell)
	}
	for _, directory := range filepath.SplitList(environmentValue(environment, "PATH")) {
		if filepath.IsAbs(directory) {
			for _, name := range []string{"pwsh.exe", "powershell.exe", "cmd.exe", "nu.exe", "bash.exe"} {
				paths = append(paths, filepath.Join(directory, name))
			}
		}
	}
	if git, found := findExecutable(`\`, environment, "git.exe"); found {
		// Git for Windows keeps git.exe in cmd, bin or mingw64\bin.
		root := filepath.Dir(filepath.Dir(git))
		if strings.HasPrefix(strings.ToLower(filepath.Base(root)), "mingw") {
			root = filepath.Dir(root)
		}
		paths = append(paths, gitShell(root)...)
	}
	for _, hive := range []registry.Key{registry.CURRENT_USER, registry.LOCAL_MACHINE} {
		for _, prefix := range []string{`SOFTWARE`, `SOFTWARE\WOW6432Node`} {
			paths = append(paths, registryShells(hive, prefix)...)
		}
	}
	absolute := paths[:0]
	for _, path := range paths {
		if filepath.IsAbs(path) {
			absolute = append(absolute, path)
		}
	}
	return absolute
}

func powershellVersions(root string) []string {
	entries, _ := os.ReadDir(root)
	paths := []string{}
	for _, entry := range entries {
		paths = append(paths, filepath.Join(root, entry.Name(), "pwsh.exe"))
	}
	return paths
}

func gitShell(root string) []string {
	for _, path := range []string{filepath.Join(root, "bin", "bash.exe"), filepath.Join(root, "usr", "bin", "bash.exe")} {
		if executable(path) {
			return []string{path}
		}
	}
	return nil
}

func additionalShells(root string) []string {
	return []string{
		filepath.Join(root, "nu", "bin", "nu.exe"),
		filepath.Join(root, "nushell", "nu.exe"),
		filepath.Join(root, "nushell", "bin", "nu.exe"),
		filepath.Join(root, "MSYS2", "usr", "bin", "bash.exe"),
		filepath.Join(root, "Cygwin", "bin", "bash.exe"),
		filepath.Join(root, "Cygwin64", "bin", "bash.exe"),
	}
}

func registryString(hive registry.Key, path, name string) (string, bool) {
	key, err := registry.OpenKey(hive, path, registry.QUERY_VALUE)
	if err != nil {
		return "", false
	}
	defer key.Close()
	value, _, err := key.GetStringValue(name)
	return value, err == nil && value != ""
}

func registryShells(hive registry.Key, prefix string) []string {
	paths := []string{}
	if root, found := registryString(hive, prefix+`\GitForWindows`, "InstallPath"); found {
		paths = append(paths, gitShell(root)...)
	}
	if root, found := registryString(hive, prefix+`\Cygwin\setup`, "rootdir"); found {
		paths = append(paths, filepath.Join(root, "bin", "bash.exe"))
	}
	for _, name := range []string{"MSYS2 64bit", "MSYS2 32bit", "MSYS2"} {
		if root, found := registryString(hive, prefix+`\Microsoft\Windows\CurrentVersion\Uninstall\`+name, "InstallLocation"); found {
			paths = append(paths, filepath.Join(root, "usr", "bin", "bash.exe"))
		}
	}
	for _, name := range []string{"pwsh.exe", "powershell.exe", "nu.exe"} {
		if path, found := registryString(hive, prefix+`\Microsoft\Windows\CurrentVersion\App Paths\`+name, ""); found {
			paths = append(paths, path)
		}
	}
	if path, found := registryString(hive, prefix+`\Microsoft\PowerShell\1\ShellIds\Microsoft.PowerShell`, "Path"); found {
		paths = append(paths, path)
	}
	installed := prefix + `\Microsoft\PowerShellCore\InstalledVersions`
	if key, err := registry.OpenKey(hive, installed, registry.ENUMERATE_SUB_KEYS); err == nil {
		versions, _ := key.ReadSubKeyNames(-1)
		key.Close()
		for _, version := range versions {
			if root, found := registryString(hive, installed+`\`+version, "InstallLocation"); found {
				paths = append(paths, filepath.Join(root, "pwsh.exe"))
			}
		}
	}
	return paths
}

// shellIdentity is the key one shell is listed under once. Launchers and
// the WSL bash stubs are not shells of this Node.
func shellIdentity(path string) (string, bool) {
	identity, err := filepath.EvalSymlinks(path)
	if err != nil {
		identity = path
	}
	if !isShell(path) || !isShell(identity) {
		return "", false
	}
	return strings.ToLower(strings.ReplaceAll(identity, "/", `\`)), true
}

func isShell(path string) bool {
	name := filepath.Base(path)
	for _, excluded := range []string{"wsl.exe", "wt.exe", "WindowsTerminal.exe", "iTerm", "iTerm2"} {
		if strings.EqualFold(name, excluded) {
			return false
		}
	}
	if !strings.EqualFold(name, "bash.exe") {
		return true
	}
	parent := filepath.Dir(path)
	directory := filepath.Base(parent)
	for _, system := range []string{"System32", "SysWOW64", "Sysnative"} {
		if strings.EqualFold(directory, system) {
			return false
		}
	}
	return !strings.EqualFold(directory, "WindowsApps") || !strings.EqualFold(filepath.Base(filepath.Dir(parent)), "Microsoft")
}

// wslShells offers each distribution through the system launcher, or the one
// on the Node's PATH.
func wslShells(environment []string, distributions []protocol.WslDistribution) []protocol.TerminalShell {
	if len(distributions) == 0 {
		return nil
	}
	launcher, found := "", false
	if system := os.Getenv("SystemRoot"); system != "" {
		launcher = filepath.Join(system, "System32", "wsl.exe")
		found = filepath.IsAbs(launcher) && executable(launcher)
	}
	if !found {
		launcher, found = findExecutable(`\`, environment, "wsl.exe")
	}
	if !found || !filepath.IsAbs(launcher) {
		return nil
	}
	shells := []protocol.TerminalShell{}
	for _, distribution := range distributions {
		shells = append(shells, protocol.TerminalShell{Path: launcher, Arguments: []string{"--distribution", distribution.Name}})
	}
	return shells
}
