// Package host selects the compile-time native Node modules shipped with
// Harness. It never acquires Desktop or renderer dependencies.
package host

import (
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	briefnode "github.com/sailry/sailry-harness/modules/brief/node"
	browsernode "github.com/sailry/sailry-harness/modules/browser/node"
	computernode "github.com/sailry/sailry-harness/modules/computer/node"
	filesnode "github.com/sailry/sailry-harness/modules/files/node"
	memorynode "github.com/sailry/sailry-harness/modules/memory/node"
	ocrnode "github.com/sailry/sailry-harness/modules/ocr/node"
	providerstatusnode "github.com/sailry/sailry-harness/modules/provider-status/node"
	remindersnode "github.com/sailry/sailry-harness/modules/reminders/node"
	scheduledtasksnode "github.com/sailry/sailry-harness/modules/scheduled-tasks/node"
	websearchnode "github.com/sailry/sailry-harness/modules/web-search/node"
)

// Harness returns the native Node assembly shared by Harness hosts.
func Harness() moduleapi.Assembly {
	return moduleapi.Assembly{
		BundledDefaults: []plugins.DefaultPackage{},
		Modules: []moduleapi.Module{
			filesnode.Module(),
			briefnode.Module(),
			ocrnode.Module(),
			websearchnode.Module(),
			memorynode.Module(),
			browsernode.Builtin(),
			browsernode.External(),
			computernode.Module(),
			providerstatusnode.Module(),
			scheduledtasksnode.Module(),
			remindersnode.Module(),
		},
	}
}
