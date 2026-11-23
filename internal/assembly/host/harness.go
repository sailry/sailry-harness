// Package host selects the compile-time native Node modules shipped with
// Harness. It never acquires Desktop or renderer dependencies.
package host

import (
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	browsernode "github.com/sailry/sailry-harness/modules/browser/node"
	codeindex "github.com/sailry/sailry-harness/modules/code-index/node"
	databasesnode "github.com/sailry/sailry-harness/modules/databases/node"
	filesnode "github.com/sailry/sailry-harness/modules/files/node"
	gitnode "github.com/sailry/sailry-harness/modules/git/node"
	memorynode "github.com/sailry/sailry-harness/modules/memory/node"
	ocrnode "github.com/sailry/sailry-harness/modules/ocr/node"
	sshnode "github.com/sailry/sailry-harness/modules/ssh/node"
	websearchnode "github.com/sailry/sailry-harness/modules/web-search/node"
	worktreesnode "github.com/sailry/sailry-harness/modules/worktrees/node"
)

// Harness returns the explicit native Node assembly for official Desktop and
// Host processes.
func Harness() moduleapi.Assembly {
	return moduleapi.Assembly{
		CodeIndex: codeindex.NewService,
		Modules: []moduleapi.Module{
			filesnode.Module(),
			ocrnode.Module(),
			gitnode.Module(),
			worktreesnode.Module(),
			codeindex.Module(),
			websearchnode.Module(),
			databasesnode.Module(),
			sshnode.Module(),
			memorynode.Module(),
			browsernode.Builtin(),
			browsernode.External(),
		},
	}
}
