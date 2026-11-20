package logging

import (
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestRemoteReportRef(t *testing.T) {
	t.Parallel()
	const ref = "e11525db-c4b2-45ff-9b82-ff23c41f1b15"
	for _, invalid := range []any{nil, "", "/private/path", protocol.NewRequest(protocol.NodeID{}, protocol.Command{}).ID.String(), "e11525db-c4b2-45ff-1b82-ff23c41f1b15", strings.ToUpper(ref), strings.ReplaceAll(ref, "-", ""), "{" + ref + "}", "urn:uuid:" + ref, true} {
		if value, ok := invalid.(string); ok && ValidReportRef(value) {
			t.Errorf("accepted invalid reference %q", value)
		}
		if safe := Remote(Fields{"phase": Failed, "report_ref": invalid}); safe["report_ref"] != nil {
			t.Errorf("invalid reference exported: %v", safe)
		}
	}
	for _, phase := range []string{Begin, Admitted, Completed, Failed, Cancelled, Interrupted} {
		safe := Remote(Fields{"phase": phase, "report_ref": ref, "request": ref, "detail": "private"})
		if (safe["report_ref"] == ref) != (phase == Failed) || safe["request"] != nil || safe["detail"] != nil {
			t.Errorf("%s fields = %v", phase, safe)
		}
	}
}
