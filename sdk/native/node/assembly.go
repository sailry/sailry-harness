package node

import (
	hostassembly "github.com/sailry/sailry-harness/internal/assembly/host"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/plugins"
)

type Assembly = moduleapi.Assembly
type DefaultPlugin = plugins.DefaultPackage

type Module = moduleapi.Module

type Host = moduleapi.Host

func HarnessAssembly() Assembly { return hostassembly.Harness() }
