package plugins

import (
	"slices"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const retainedNodeReports = 64

type nodeReport struct {
	ref  string
	code protocol.ErrorCode
}

func (binding *NativeBinding) rememberNodeReport(err error) {
	ref, owned := client.DeliveredReport(err)
	if !owned || !logging.ValidReportRef(ref) {
		return
	}
	entry := nodeReport{ref: ref, code: hostFault(err).Code}
	state := binding.state
	state.mu.Lock()
	defer state.mu.Unlock()
	if state.ctx.Err() != nil || slices.Contains(state.nodeReports, entry) {
		return
	}
	if len(state.nodeReports) == retainedNodeReports {
		copy(state.nodeReports, state.nodeReports[1:])
		state.nodeReports = state.nodeReports[:retainedNodeReports-1]
	}
	state.nodeReports = append(state.nodeReports, entry)
}

// OwnsNodeReport checks a toast claim against replies delivered to this view.
func (binding *NativeBinding) OwnsNodeReport(ref string, code protocol.ErrorCode) bool {
	if ref == "" || code == "" {
		return false
	}
	state := binding.state
	state.mu.Lock()
	defer state.mu.Unlock()
	return state.ctx.Err() == nil && slices.Contains(state.nodeReports, nodeReport{ref: ref, code: code})
}
