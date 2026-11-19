package plugins

import (
	"maps"
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// snapshot detaches a validated contribution for presentation without replaying
// its wire codecs each time the composer draws the same published metric.
func (entry ContributionEntry) snapshot() ContributionEntry {
	entry.Key.Session = clonePointer(entry.Key.Session)
	entry.Key.Worktree = clonePointer(entry.Key.Worktree)
	d := &entry.Declaration
	d.Label = cloneLabel(d.Label)
	d.Intent, d.Icon, d.Handler = clonePointer(d.Intent), clonePointer(d.Icon), clonePointer(d.Handler)
	d.Width, d.Hover, d.Command = clonePointer(d.Width), clonePointer(d.Hover), clonePointer(d.Command)
	d.Choices = cloneChoices(d.Choices)
	s := &entry.State
	s.Scope, s.Icon, s.ReplyTo = clonePointer(s.Scope), clonePointer(s.Icon), clonePointer(s.ReplyTo)
	s.Value, s.Label = slices.Clone(s.Value), cloneOptionalLabel(s.Label)
	if s.Choices != nil {
		choices := cloneChoices(*s.Choices)
		s.Choices = &choices
	}
	s.Details = slices.Clone(s.Details)
	for i := range s.Details {
		detail := &s.Details[i]
		detail.Label, detail.Progress = cloneLabel(detail.Label), clonePointer(detail.Progress)
	}
	s.Segments = slices.Clone(s.Segments)
	for i := range s.Segments {
		s.Segments[i].Label = cloneLabel(s.Segments[i].Label)
	}
	return entry
}

func clonePointer[T any](value *T) *T {
	if value == nil {
		return nil
	}
	cloned := *value
	return &cloned
}

func cloneLabel(label protocol.PluginDesktopNavigation) protocol.PluginDesktopNavigation {
	label.Locales = maps.Clone(label.Locales)
	if label.Icon != nil {
		// Icons are a tagged wire union with a pointer-valued payload.
		cloned := contributionCopy(*label.Icon)
		label.Icon = &cloned
	}
	return label
}

func cloneOptionalLabel(label *protocol.PluginDesktopNavigation) *protocol.PluginDesktopNavigation {
	if label == nil {
		return nil
	}
	cloned := cloneLabel(*label)
	return &cloned
}

func cloneChoices(choices []protocol.PluginUiChoice) []protocol.PluginUiChoice {
	cloned := slices.Clone(choices)
	for i := range cloned {
		choice := &cloned[i]
		choice.Label, choice.Description, choice.Group = cloneLabel(choice.Label), cloneOptionalLabel(choice.Description), cloneOptionalLabel(choice.Group)
		choice.Icon, choice.Children = clonePointer(choice.Icon), cloneChoices(choice.Children)
	}
	return cloned
}
