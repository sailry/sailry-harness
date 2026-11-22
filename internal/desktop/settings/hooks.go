package settings

import "github.com/sailry/sailry-harness/internal/desktop/preview"

// HookSettings are a Node's hooks: on or off, and their scripts.
type HookSettings struct {
	Enabled bool
	Scripts []preview.Hook
}

type HookTarget struct {
	Node, Host string
}
