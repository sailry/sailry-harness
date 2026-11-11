package process

import "github.com/sailry/sailry-harness/internal/protocol"

// A Windows command lives in a kill-on-close Job: the system ends it when
// the Node's last handle closes, a crash included, so nothing is recorded.
func recordGroup(string, protocol.RequestID, int) func() { return func() {} }

func reapGroups(string) error { return nil }
