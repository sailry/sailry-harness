package conversation

import "strings"

// Grouping rules as pure functions over the view models so tests can check
// them without a view.

// groupable reports a call that joins the calls around it into one block.
func groupable(c Call) bool { return !c.Terminal && !c.Standalone }

// Compact prepares a reply's blocks for display: blank text goes, calls
// that never group get blocks of their own, and adjacent tool blocks, and
// adjacent children blocks, join.
func Compact(blocks []Block) []Block {
	var out []Block
	push := func(b Block) {
		if n := len(out); n > 0 {
			last := &out[n-1]
			switch {
			case b.Kind == ToolsBlock && last.Kind == ToolsBlock && groupable(b.Calls[0]) && groupable(last.Calls[len(last.Calls)-1]):
				last.Calls = append(last.Calls[:len(last.Calls):len(last.Calls)], b.Calls...)
				return
			case b.Kind == ChildrenBlock && last.Kind == ChildrenBlock:
				last.Children = append(last.Children[:len(last.Children):len(last.Children)], b.Children...)
				return
			}
		}
		out = append(out, b)
	}
	for _, b := range blocks {
		switch {
		case b.Kind == TextBlock && strings.TrimSpace(b.Text) == "":
		case b.Kind == ToolsBlock:
			for _, c := range b.Calls {
				push(Block{Kind: ToolsBlock, Calls: []Call{c}})
			}
		default:
			push(b)
		}
	}
	return out
}

// work reports a block that belongs in the collapsible work region:
// everything but text, questions, the answer's pictures, files and
// visuals, the notice that the answer was cut, and tool blocks of
// interactions alone.
func work(b Block) bool {
	switch b.Kind {
	case TextBlock, QuestionBlock, ImagesBlock, FilesBlock, VisualBlock, LimitBlock:
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

// Summary is the header of a group: the last call's label and detail, with
// the states of all of them (group_summary).
func Summary(calls []Call) Call {
	s := calls[len(calls)-1]
	s.Output = nil
	s.Pending, s.Running, s.Failed, s.Warning = false, false, false, false
	for _, c := range calls {
		s.Pending = s.Pending || c.Pending
		s.Running = s.Running || c.Running
	}
	active := s.Pending || s.Running
	for _, c := range calls {
		if !active {
			s.Failed = s.Failed || c.Failed
			s.Warning = s.Warning || c.Warning
		}
	}
	if s.Failed {
		s.Warning = false
	}
	if active {
		s.State, s.Background = StateNone, nil
	}
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
