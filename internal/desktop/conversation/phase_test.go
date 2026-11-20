package conversation

import "testing"

// The model's own progress shows only before the reply's first output;
// waiting on children still shows after it.
func TestThinkingFallbackFollowsVisibleActivity(t *testing.T) {
	tool := Block{Kind: ToolsBlock, Calls: []Call{{ID: "read", Running: true}}}
	text := Block{Kind: TextBlock, Text: "Answer"}
	reasoning := Block{Kind: ReasoningBlock, Text: "Checking"}
	for _, tc := range []struct {
		name           string
		phase          Phase
		blocks         []Block
		expanded, want bool
	}{
		{"quiet", AwaitingResponse, nil, true, true},
		{"running tool", ExecutingTool, []Block{tool}, true, false},
		{"folded tool", ExecutingTool, []Block{tool}, false, false},
		{"streaming answer", Generating, []Block{text}, true, false},
		{"streaming reasoning", Thinking, []Block{reasoning}, true, false},
		{"folded reasoning", Thinking, []Block{reasoning}, false, false},
		{"waiting after committed answer", AwaitingResponse, []Block{text}, true, false},
		{"waiting after prior reasoning", AwaitingResponse, []Block{reasoning}, true, false},
		{"waiting after a tool", AwaitingResponse, []Block{{Kind: ToolsBlock, Calls: []Call{{ID: "done"}}}}, false, false},
		{"waiting on children after output", WaitingChildren, []Block{text}, true, true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			reply := Reply{Status: Running, Phase: tc.phase, Blocks: tc.blocks}
			if got := showPhase(&reply, partitionReply(tc.blocks, tc.expanded)); got != tc.want {
				t.Fatalf("showPhase = %v, want %v", got, tc.want)
			}
		})
	}
}

func TestToolSweepFollowsVisibleRow(t *testing.T) {
	blocks := []Block{{Kind: ToolsBlock, Calls: []Call{{ID: "read", Running: true}}}}
	if !partitionReply(blocks, true).runningToolVisible() || partitionReply(blocks, false).runningToolVisible() {
		t.Fatal("the visible tool row should own the sweep only while expanded")
	}
}
