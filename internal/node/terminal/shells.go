package terminal

import (
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Shells discovers the shells a terminal can start on this Node, ordered by
// path. distributions are the WSL distributions to offer through wsl.exe;
// they only apply on Windows.
func Shells(environment []string, distributions []protocol.WslDistribution) []protocol.TerminalShell {
	shells := []protocol.TerminalShell{}
	seen := map[string]bool{}
	for _, path := range shellCandidates(environment) {
		identity, valid := shellIdentity(path)
		if !valid || !executable(path) || seen[identity] {
			continue
		}
		seen[identity] = true
		shells = append(shells, protocol.TerminalShell{Path: path, Arguments: []string{}})
	}
	shells = append(shells, wslShells(environment, distributions)...)
	slices.SortFunc(shells, func(first, second protocol.TerminalShell) int {
		if order := strings.Compare(first.Path, second.Path); order != 0 {
			return order
		}
		return slices.Compare(first.Arguments, second.Arguments)
	})
	return shells
}

func environmentValue(environment []string, name string) string {
	found := ""
	for _, entry := range environment {
		if key, value, valid := strings.Cut(entry, "="); valid && strings.EqualFold(key, name) {
			found = value
		}
	}
	return found
}
