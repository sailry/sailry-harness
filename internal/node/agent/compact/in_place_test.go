package compact

import (
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestInPlaceInstructions(t *testing.T) {
	whole := InPlace(200000, 16000, nil)
	if strings.Contains(whole, "Conversation History") || strings.Contains(whole, "{{") ||
		!strings.Contains(whole, "replaces the conversation above") || !strings.Contains(whole, "## What to keep") || !strings.Contains(whole, "```json") {
		t.Fatalf("in-place instructions lost their task or shared rules: %q", whole)
	}
	if strings.Contains(whole, "kept verbatim") {
		t.Fatal("a summary without a retained tail named one")
	}
	turn := protocol.NewTurnID()
	input := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, "Fix the \"parser\"\n  and "+strings.Repeat("keep going ", 30))), time.Now())
	input.Metadata.Runtime = &canonical.Runtime{UserVisible: true, AgentVisible: true}
	retained := InPlace(200000, 16000, []canonical.Record{input})
	if !strings.Contains(retained, `begins "Fix the 'parser' and keep going`) || !strings.Contains(retained, "…\" onward are kept verbatim") {
		t.Fatalf("retained tail was not quoted on one bounded line: %q", retained)
	}
	// A tail cut within a turn begins after its user input.
	reply := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Parser fixed")), time.Now())
	reply.Metadata.Runtime = &canonical.Runtime{UserVisible: true, AgentVisible: true}
	if within := InPlace(200000, 16000, []canonical.Record{reply, input}); !strings.Contains(within, `assistant message that begins "Parser fixed"`) {
		t.Fatalf("a tail within a turn was not quoted from its first message: %q", within)
	}
}
