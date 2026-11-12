package plugins

import (
	"encoding/hex"

	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func ToolAlias(packageName, tool string) string {
	digest := blake3.Sum256([]byte(packageName))
	return "plugin_" + hex.EncodeToString(digest[:8]) + "_" + tool
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
