package live

import (
	"context"
	"sync"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type skillSourceOperation struct {
	ctx    context.Context
	cancel context.CancelFunc
	mu     sync.Mutex
	stream protocol.StreamID
}

func (r *Runtime) cancelSkillSource() {
	if operation := r.packages.skillOperation; operation != nil {
		r.finishSkillSource(operation, false)
	}
}

func (r *Runtime) finishSkillSource(operation *skillSourceOperation, retained bool) {
	if operation == nil {
		return
	}
	if r.packages.skillOperation == operation {
		r.packages.skillOperation = nil
	}
	operation.mu.Lock()
	stream := operation.stream
	operation.stream = protocol.StreamID{}
	operation.mu.Unlock()
	operation.cancel()
	if !retained && stream != (protocol.StreamID{}) {
		r.async(func() { r.cancelAttachmentStream(stream) })
	}
}
