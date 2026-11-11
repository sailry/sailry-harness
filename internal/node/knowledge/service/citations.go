package service

import (
	"context"
	"encoding/json"
	"path/filepath"
	"strings"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/node/knowledge"
	"github.com/sailry/sailry-harness/internal/node/knowledge/source"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func citationCancelled(ctx context.Context) error {
	if ctx.Err() != nil {
		return newFault(protocol.ErrorCodeCancelled, "knowledge citation read was cancelled")
	}
	return nil
}

func (service *Service) permitted(ctx context.Context, request protocol.Request, collection protocol.KnowledgeID) (store.KnowledgeStored, error) {
	allowed, err := service.options.Allowed(ctx, request)
	if err != nil {
		return store.KnowledgeStored{}, err
	}
	for _, stored := range allowed {
		if stored.Collection.ID == collection {
			return stored, nil
		}
	}
	return store.KnowledgeStored{}, newFault(protocol.ErrorCodePermissionDenied, "knowledge collection is not accessible")
}

// Citation rereads a cited source through the current grants. A citation
// whose collection, source or chunk changed is reported and reindexed.
func (service *Service) Citation(ctx context.Context, request protocol.Request, input protocol.KnowledgeCitationRequest) (protocol.KnowledgeCitationPage, error) {
	none := protocol.DefaultKnowledgeCitationPage()
	if err := knowledge.ValidateCitation(input); err != nil {
		return none, err
	}
	if err := citationCancelled(ctx); err != nil {
		return none, err
	}
	stored, err := service.permitted(ctx, request, input.Collection)
	if err != nil {
		return none, err
	}
	choice := service.options.Choice()
	indexed, err := service.index.citation(ctx, service.binding(stored, choice), input)
	if err != nil {
		if code := code(err); code == protocol.ErrorCodeRevisionConflict || code == protocol.ErrorCodeNotFound {
			service.Refresh()
		}
		return none, err
	}
	if err := citationCancelled(ctx); err != nil {
		return none, err
	}
	definition := stored.Collection.Definition
	selected := store.ReadKnowledgeSource(definition.Source)
	root := ""
	var worktree *protocol.WorktreeID
	if selected.Kind == "worktree" {
		if root, err = service.options.Root(ctx, selected.Worktree); err != nil {
			return none, err
		}
		worktree = &selected.Worktree
	}
	var text string
	if selected.Kind == "storage" {
		text, err = service.value(ctx, stored, indexed.citation)
	} else {
		text, err = service.context(ctx, definition, selected, root, indexed.citation, input.Context)
	}
	if err != nil {
		if code(err) == protocol.ErrorCodeRevisionConflict {
			service.Refresh()
		}
		return none, err
	}
	if worktree != nil {
		moved, err := service.options.Root(ctx, selected.Worktree)
		if err != nil {
			return none, err
		}
		if moved != root {
			return none, changedSource()
		}
	}
	if err := citationCancelled(ctx); err != nil {
		return none, err
	}
	current, err := service.permitted(ctx, request, input.Collection)
	if err != nil {
		return none, err
	}
	if !store.SameKnowledgeStored(current, stored) || !sameChoice(service.options.Choice(), choice) {
		return none, changedSource()
	}
	return page(indexed, text, input, worktree)
}

func sameChoice(left, right protocol.ModelsLocalChoice) bool {
	return left.Enabled == right.Enabled && (left.Model == nil) == (right.Model == nil) && (left.Model == nil || *left.Model == *right.Model)
}

func sourceFault(err error) error {
	if code(err) == protocol.ErrorCodeNotFound {
		return changedSource()
	}
	return err
}

// within returns the slash-separated path of locator below base.
func within(base, locator string) (string, bool) {
	relative, err := filepath.Rel(base, locator)
	if err != nil || relative == "." || relative == ".." || strings.HasPrefix(relative, ".."+string(filepath.Separator)) {
		return "", false
	}
	return filepath.ToSlash(relative), true
}

// context reads the cited text, or the complete document, from the source
// as it is now.
func (service *Service) context(ctx context.Context, definition protocol.KnowledgeDefinition, selected store.KnowledgeSource, root string, citation protocol.KnowledgeCitation, complete bool) (string, error) {
	var scan *source.Scan
	var relative string
	var err error
	switch selected.Kind {
	case "worktree":
		relative = citation.Path
		if selected.Path != "." && selected.Path != "" && citation.Path != selected.Path && !strings.HasPrefix(citation.Path, selected.Path+"/") {
			return "", changedSource()
		}
		scan, err = source.Worktree(ctx, root, citation.Path)
	case "directory":
		found := false
		if relative, found = within(selected.Path, citation.Path); !found {
			return "", changedSource()
		}
		scan, err = source.Within(ctx, selected.Path, relative)
	case "file":
		if selected.Path != citation.Path {
			return "", changedSource()
		}
		relative = filepath.Base(citation.Path)
		scan, err = source.Controller(ctx, "file", selected.Path)
	default:
		return "", invalid("file citation expected")
	}
	if err != nil {
		return "", sourceFault(err)
	}
	defer scan.Close()
	matched, err := matches(definition.Globs, relative)
	if err != nil {
		return "", err
	}
	if !matched {
		return "", changedSource()
	}
	scan.SetOCR(service.options.OCR)
	entry, err := scan.Next(func(string, source.Stat) (bool, error) { return false, nil }, func([]source.Watch) {})
	if err != nil {
		return "", err
	}
	if entry != nil && entry.Failed {
		return "", newFault(protocol.ErrorCodeUnavailable, entry.Reason)
	}
	if entry == nil || entry.Document == nil || entry.Document.Path != citation.Path || entry.Document.Revision != citation.Revision {
		return "", changedSource()
	}
	if complete {
		texts := make([]string, len(entry.Document.Parts))
		for index, part := range entry.Document.Parts {
			texts[index] = part.Text
		}
		return strings.Join(texts, "\n\n"), nil
	}
	return position(entry.Document.Parts, citation.Position)
}

// matches reports a path one of the globs selects; no globs select all.
func matches(globs []string, relative string) (bool, error) {
	if len(globs) == 0 {
		return true, nil
	}
	matched := false
	for _, pattern := range globs {
		glob, err := knowledge.Glob(pattern)
		if err != nil {
			return false, err
		}
		matched = matched || glob.MatchString(relative)
	}
	return matched, nil
}

// lines reads a line range position.
func lines(position protocol.KnowledgePosition) (start, end uint32, found bool) {
	if position.Kind != "lines" {
		return 0, 0, false
	}
	var payload protocol.KnowledgePositionLinesPayload
	if err := json.Unmarshal([]byte(encoded(position.Data)), &payload); err != nil {
		return 0, 0, false
	}
	return payload.Start, payload.End, true
}

func position(parts []source.Part, selected protocol.KnowledgePosition) (string, error) {
	if start, end, found := lines(selected); found {
		if start == 0 || end < start {
			return "", changedSource()
		}
		for _, part := range parts {
			if part.Position.Kind != "lines" {
				continue
			}
			text := source.Lines(part.Text)
			if int(end) > len(text) {
				return "", changedSource()
			}
			return strings.Join(text[start-1:end], "\n"), nil
		}
		return "", changedSource()
	}
	wanted := encoded(selected)
	var texts []string
	for _, part := range parts {
		if encoded(part.Position) == wanted {
			texts = append(texts, part.Text)
		}
	}
	if len(texts) == 0 {
		return "", changedSource()
	}
	return strings.Join(texts, "\n\n"), nil
}

func page(indexed record, text string, input protocol.KnowledgeCitationRequest, worktree *protocol.WorktreeID) (protocol.KnowledgeCitationPage, error) {
	start := int(input.Offset)
	if start > len(text) || start < len(text) && !utf8.RuneStart(text[start]) {
		return protocol.DefaultKnowledgeCitationPage(), invalid("knowledge citation offset is not a text boundary")
	}
	end := min(start+int(min(input.Limit, knowledge.CitationLimit)), len(text))
	for end < len(text) && !utf8.RuneStart(text[end]) {
		end--
	}
	if start < len(text) && start == end {
		return protocol.DefaultKnowledgeCitationPage(), invalid("knowledge citation limit cannot fit the next character")
	}
	page := protocol.KnowledgeCitationPage{
		Citation:  indexed.citation,
		Title:     indexed.title,
		Text:      text[start:end],
		Offset:    input.Offset,
		Worktree:  worktree,
		Context:   input.Context,
		Untrusted: true,
	}
	if end < len(text) {
		next := uint32(end)
		page.NextOffset = &next
	}
	return page, nil
}
