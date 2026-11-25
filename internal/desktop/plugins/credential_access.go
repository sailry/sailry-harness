package plugins

import "github.com/sailry/sailry-harness/internal/protocol"

// ResolveCredential is for native bindings. Only a reference, not the
// credential itself, is returned to a view script.
func (binding *NativeBinding) ResolveCredential(identity string) (protocol.Secret, error) {
	return binding.secret(identity)
}
