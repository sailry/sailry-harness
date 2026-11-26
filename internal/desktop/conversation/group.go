package conversation

import "strings"

// Grouping rules as pure functions over the view models so tests can check
// them without a view.

// groupable reports a call that joins the calls around it into one block.
// A tool's declaration chooses it (grouping: standalone sets Standalone).
func groupable(c Call) bool { return !c.Terminal && !c.Standalone }

// Compact prepares a reply's blocks for display: blank text goes, calls
// that never group get blocks of their own, and adjacent tool blocks, and
// adjacent children blocks, join. The task's progress card keeps its place
// without splitting the run of calls around it: later calls join the block
// before it, so a growing run never adds a second block below the card.
// Failures stay at the end of the answer, outside the work region even when
// later work or children were projected.
// isCompact reports whether blocks are already in the form Compact produces,
// allowing Compact to reuse them without allocating.
func isCompact(blocks []Block) bool {
	seenError := false
	canJoinTools := false
	for i := range blocks {
		b := &blocks[i]
		if seenError && b.Kind != ErrorBlock {
			return false
		}
		switch b.Kind {
		case ErrorBlock:
			seenError = true
			canJoinTools = false
		case TextBlock:
			if strings.TrimSpace(b.Text) == "" {
				return false
			}
			canJoinTools = false
		case ToolsBlock:
			if len(b.Calls) == 0 {
				return false
			}
			if len(b.Calls) == 1 {
				g := groupable(b.Calls[0])
				if canJoinTools && g {
					return false
				}
				canJoinTools = g
				continue
			}
			if canJoinTools {
				return false
			}
			for j := range b.Calls {
				if !groupable(b.Calls[j]) {
					return false
				}
			}
			canJoinTools = true
		case ChildrenBlock:
			if len(b.Calls) > 0 || (b.Children != nil && len(b.Children) == 0) || (i+1 < len(blocks) && blocks[i+1].Kind == ChildrenBlock) {
				return false
			}
			canJoinTools = false
		case ProgressBlock:
		default:
			canJoinTools = false
		}
	}
	return true
}

// groupableRunLen counts consecutive groupable calls starting at
// blocks[blockIdx].Calls[callIdx] that will merge into one ToolsBlock.
func groupableRunLen(blocks []Block, blockIdx, callIdx int) int {
	count := 0
	for j := callIdx; j < len(blocks[blockIdx].Calls); j++ {
		if !groupable(blocks[blockIdx].Calls[j]) {
			return count
		}
		count++
	}
	for i := blockIdx + 1; i < len(blocks); i++ {
		b := &blocks[i]
		switch b.Kind {
		case ProgressBlock:
			continue
		case TextBlock:
			if strings.TrimSpace(b.Text) == "" {
				continue
			}
			return count
		case ToolsBlock:
			for j := range b.Calls {
				if !groupable(b.Calls[j]) {
					return count
				}
				count++
			}
		default:
			return count
		}
	}
	return count
}

func Compact(blocks []Block) []Block {
	if len(blocks) == 0 {
		return nil
	}
	if isCompact(blocks) {
		return blocks[:len(blocks):len(blocks)]
	}
	var out []Block
	var failures []Block
	joinFrom := 0
	appendChildCall := func(c Call, hint int, solo []Call) {
		n := len(out)
		for n > joinFrom && out[n-1].Kind == ProgressBlock {
			n--
		}
		if n > joinFrom {
			last := &out[n-1]
			if last.Kind == ToolsBlock && groupable(c) && groupable(last.Calls[len(last.Calls)-1]) {
				last.Calls = append(last.Calls, c)
				return
			}
		}
		if !groupable(c) {
			out = append(out, Block{Kind: ToolsBlock, Calls: solo})
			return
		}
		if hint < 1 {
			hint = 1
		}
		calls := make([]Call, 1, hint)
		calls[0] = c
		out = append(out, Block{Kind: ToolsBlock, Calls: calls})
	}
	for i := 0; i < len(blocks); i++ {
		b := blocks[i]
		switch {
		case b.Kind == ErrorBlock:
			failures = append(failures, b)
			joinFrom = len(out)
		case b.Kind == TextBlock && strings.TrimSpace(b.Text) == "":
		case b.Kind == ToolsBlock:
			for j, c := range b.Calls {
				if !groupable(c) {
					out = append(out, Block{Kind: ToolsBlock, Calls: b.Calls[j : j+1 : j+1]})
					continue
				}
				n := len(out)
				for n > joinFrom && out[n-1].Kind == ProgressBlock {
					n--
				}
				if n > joinFrom {
					last := &out[n-1]
					if last.Kind == ToolsBlock && groupable(last.Calls[len(last.Calls)-1]) {
						last.Calls = append(last.Calls, c)
						continue
					}
				}
				run := groupableRunLen(blocks, i, j)
				if j == 0 && run == len(b.Calls) {
					out = append(out, Block{Kind: ToolsBlock, Calls: b.Calls[:len(b.Calls):len(b.Calls)]})
					break
				}
				calls := make([]Call, 1, run)
				calls[0] = c
				out = append(out, Block{Kind: ToolsBlock, Calls: calls})
			}
		case b.Kind == ChildrenBlock:
			// Consecutive launches share one count after their original call
			// disclosures. A later launch starts a new count, not a session total.
			joinFrom = len(out)
			var children []Child
			for {
				for j, c := range b.Calls {
					appendChildCall(c, len(b.Calls)-j, b.Calls[j:j+1:j+1])
				}
				children = append(children, b.Children...)
				if i+1 >= len(blocks) || blocks[i+1].Kind != ChildrenBlock {
					break
				}
				i++
				b = blocks[i]
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
	lines := strings.Split(strings.TrimSuffix(text, "\n"), "\n")
	if len(lines) <= 100 {
		return strings.Join(lines, "\n"), 0
	}
	return strings.Join(lines[len(lines)-100:], "\n"), len(lines) - 100
}

// previewText is what a closed preview of lines shows: a command's last
// previewLines, as its end is what it came to, and another text's first.
// Shown in full it is at most expandedLines. hidden is how many lines are
// left out.
func previewText[T any](lines []T, full, command bool) (shown []T, hidden int) {
	limit := previewLines
	if full {
		limit = expandedLines
	}
	if len(lines) <= limit {
		return lines, 0
	}
	if command {
		return lines[len(lines)-limit:], len(lines) - limit
	}
	return lines[:limit], len(lines) - limit
}
