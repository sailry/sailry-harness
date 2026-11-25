package node

import "github.com/sailry/sailry-harness/internal/node/plugins"

func legacyNativeToolPackage(name, digest string) bool {
	return plugins.IsLegacyNativeToolPackage(name, digest)
}
