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

type replySections struct {
	work, answer, failures []Block
	hasWork, endsInRetry   bool
}

func (p replySections) runningToolVisible() bool {
	for _, section := range [][]Block{p.work, p.answer} {
		for _, block := range section {
			if block.Kind == ToolsBlock {
				for _, call := range block.Calls {
					if call.Running {
						return true
					}
				}
			}
		}
	}
	return false
}

// partitionReply only groups visible blocks. Error blocks always escape the
// work fold, as do terminal calls following the last nonterminal call.
func partitionReply(blocks []Block, expanded bool) replySections {
	var parts replySections
	var visible []Block
	if expanded {
		visible = Compact(blocks)
		start := AnswerFrom(visible)
		parts.work = visible[:start]
		parts.hasWork = start > 0
		parts.endsInRetry = start > 0 && visible[start-1].Kind == RetryBlock
		visible = visible[start:]
	} else {
		start := AnswerFrom(blocks)
		parts.hasWork = start > 0
		parts.endsInRetry = start > 0 && blocks[start-1].Kind == RetryBlock
		for i := 0; i < start; i++ {
			if blocks[i].Kind == ErrorBlock {
				parts.failures = append(parts.failures, blocks[i])
			}
		}
		if start > 0 && blocks[start-1].Kind == ToolsBlock {
			calls := blocks[start-1].Calls
			first := len(calls)
			for first > 0 && calls[first-1].Terminal {
				first--
			}
			for i := first; i < len(calls); i++ {
				visible = append(visible, Block{Kind: ToolsBlock, Calls: calls[i : i+1 : i+1]})
			}
		}
		tail := Compact(blocks[start:])
		if len(visible) == 0 {
			visible = tail
		} else {
			visible = append(visible, tail...)
		}
	}
	end := len(visible)
	for end > 0 && visible[end-1].Kind == ErrorBlock {
		end--
	}
	parts.answer = visible[:end]
	if len(parts.failures) == 0 {
		parts.failures = visible[end:]
	} else {
		parts.failures = append(parts.failures, visible[end:]...)
	}
	parts.endsInRetry = parts.endsInRetry && len(parts.answer) == 0 && len(parts.failures) == 0
	return parts
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

// pathList reports a call whose body lists the paths it touched: two or
// more, or one other than its detail.
func pathList(c Call) bool {
	if c.Output == nil {
		return false
	}
	n := len(c.Output.Paths)
	return n >= 2 || n == 1 && c.Output.Paths[0] != c.Detail
}

// awaitingModel is a call whose row sweeps while the model's next output is
// awaited: a foreground call of the returned batch that did not fail, is
// not awaiting approval and takes no warning color.
func awaitingModel(call Call) bool {
	return call.AwaitingModel && call.Background == nil && !call.Failed && !call.Pending && !flagged(call)
}

// uniquePaths lists paths once each, in order: a path row is keyed by its
// path.
func uniquePaths(paths []string) []string {
	seen := map[string]bool{}
	var out []string
	for _, p := range paths {
		if !seen[p] {
			seen[p] = true
			out = append(out, p)
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
