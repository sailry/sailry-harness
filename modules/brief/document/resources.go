// Package document owns the embedded Brief document renderer. The controller
// supplies the shared offline visual libraries and sandbox; they are not part
// of this embed.
package document

import "github.com/sailry/sailry-harness/internal/protocol"

// Reference returns the renderer identity written to new artifact revisions.
// Keep older resources registered when selecting a new renderer for writes.
func Reference() protocol.PluginReference { return referenceV1 }
