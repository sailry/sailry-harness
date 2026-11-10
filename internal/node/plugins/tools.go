package plugins

import (
	"encoding/hex"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func ToolAlias(packageName, tool string) string {
	digest := blake3.Sum256([]byte(packageName))
	return "plugin_" + hex.EncodeToString(digest[:8]) + "_" + tool
}

func IsToolAlias(name string) bool {
	suffix, found := strings.CutPrefix(name, "plugin_")
	if !found {
		return false
	}
	digest, tool, found := strings.Cut(suffix, "_")
	if !found || len(digest) != 16 || tool == "" {
		return false
	}
	for _, character := range digest {
		if !(character >= '0' && character <= '9' || character >= 'a' && character <= 'f') {
			return false
		}
	}
	return true
}

func ToolOperation(info protocol.PluginInfo, name string) *protocol.ToolOperation {
	if info.Extension == nil {
		return nil
	}
	for _, tool := range info.Extension.Tools {
		if ToolAlias(info.Summary.Name, tool.Name) != name {
			continue
		}
		if tool.Operation != nil {
			return tool.Operation
		}
		if tool.Handler != nil {
			return tool.Handler.Operation
		}
	}
	return nil
}
