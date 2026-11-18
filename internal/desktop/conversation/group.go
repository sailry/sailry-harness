package conversation

import "strings"

// Grouping rules as pure functions over the view models so tests can check
// them without a view.

// groupable reports a call that joins the calls around it into one block.
func groupable(c Call) bool { return !c.Terminal && !c.Standalone }

// Compact prepares a reply's blocks for display: blank text goes, calls
// that never group get blocks of their own, and adjacent tool blocks, and
// adjacent children blocks, join. Failures stay at the end of the answer,
// outside the work region even when later work or children were projected.
func Compact(blocks []Block) []Block {
	var out []Block
	var failures []Block
	joinFrom := 0
	pushCall := func(c Call) {
		if n := len(out); n > joinFrom {
			last := &out[n-1]
			if last.Kind == ToolsBlock && groupable(c) && groupable(last.Calls[len(last.Calls)-1]) {
				last.Calls = append(last.Calls, c)
				return
			}
		}
		out = append(out, Block{Kind: ToolsBlock, Calls: []Call{c}})
	}
	for len(blocks) > 0 {
		b := blocks[0]
		blocks = blocks[1:]
		switch {
		case b.Kind == ErrorBlock:
			failures = append(failures, b)
			joinFrom = len(out)
		case b.Kind == TextBlock && strings.TrimSpace(b.Text) == "":
		case b.Kind == ToolsBlock:
			for _, c := range b.Calls {
				pushCall(c)
			}
		case b.Kind == ChildrenBlock:
			// Consecutive launches share one count after their original call
			// disclosures. A later launch starts a new count, not a session total.
			joinFrom = len(out)
			var children []Child
			for {
				for _, c := range b.Calls {
					pushCall(c)
				}
				children = append(children, b.Children...)
				if len(blocks) == 0 || blocks[0].Kind != ChildrenBlock {
					break
				}
				b, blocks = blocks[0], blocks[1:]
			}
			out = append(out, Block{Kind: ChildrenBlock, Children: children})
		default:
			out = append(out, b)
		}
	}
	return append(out, failures...)
}

// work reports a block that belongs in the collapsible work region:
// everything but text, questions, the answer's pictures, files and
// visuals, failures, the notice that the answer was cut, and tool blocks of
// interactions alone.
func work(b Block) bool {
	switch b.Kind {
	case TextBlock, QuestionBlock, ImagesBlock, FilesBlock, VisualBlock, ErrorBlock, LimitBlock:
		return false
	case ToolsBlock:
		for _, c := range b.Calls {
			if !c.Terminal {
				return true
			}
		}
		return false
	}
	return true
}

// AnswerFrom is the index of the first block after the last work block:
// the blocks before it fold into the work region, the rest are the answer.
func AnswerFrom(blocks []Block) int {
	for i := len(blocks) - 1; i >= 0; i-- {
		if work(blocks[i]) {
			return i + 1
		}
	}
	return 0
}

// Runs splits calls into runs of the same name, as tool_groups does.
func Runs(calls []Call) [][]Call {
	var out [][]Call
	for i, c := range calls {
		if i > 0 && calls[i-1].Name == c.Name {
			out[len(out)-1] = append(out[len(out)-1], c)
			continue
		}
		out = append(out, []Call{c})
	}
	return out
}

// Sequence reports calls that go under one outer disclosure: more than one,
// not all of the same name.
func Sequence(calls []Call) bool {
	return len(calls) > 1 && len(Runs(calls)) > 1
}

// pathList reports a call whose body lists the paths it touched: two or
// more, or one other than its detail.
func pathList(c Call) bool {
	if c.Output == nil {
		return false
	}
	n := len(c.Output.Paths)
	return n >= 2 || n == 1 && c.Output.Paths[0] != c.Detail
}

// Summary shows the last call's label, detail and status, without its output.
func Summary(calls []Call) Call {
	s := calls[len(calls)-1]
	s.Output = nil
	return s
}

// uniquePaths lists the paths of calls once each, in order.
func uniquePaths(calls []Call) []string {
	seen := map[string]bool{}
	var out []string
	for _, c := range calls {
		if c.Output == nil {
			continue
		}
		for _, p := range c.Output.Paths {
			if !seen[p] {
				seen[p] = true
				out = append(out, p)
			}
		}
	}
	return out
}

// lastLines keeps a command's last 100 lines, returning how many it hid.
func lastLines(text string) (string, int) {
	lines := strings.Split(strings.TrimRight(text, "\n"), "\n")
	if len(lines) <= 100 {
		return strings.Join(lines, "\n"), 0
	}
	return strings.Join(lines[len(lines)-100:], "\n"), len(lines) - 100
}
