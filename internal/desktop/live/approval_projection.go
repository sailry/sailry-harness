package live

import (
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func projectApproval(approval protocol.ConversationApproval, call *protocol.ConversationPartToolCallPayload, locale string) composer.Approval {
	view := composer.Approval{ID: approval.ID.String(), Remember: approval.CanRemember, Unrestricted: approval.Access != nil && *approval.Access == protocol.ProcessAccessUnrestricted}
	if approval.Prefix != nil {
		view.Prefix = *approval.Prefix
	}
	if trust := approval.Ssh; trust != nil {
		view.Trust = &composer.Trust{Host: trust.Host, Port: int(trust.Port), Algorithm: trust.Key.Algorithm, Fingerprint: trust.Key.Fingerprint, Changed: trust.Changed}
	}
	if call != nil {
		view.Title = call.Name
		if call.Summary != nil && call.Summary.Resolved != nil {
			resolved := call.Summary.Resolved
			if resolved.Label != nil {
				view.Title = toolLocalText(*resolved.Label, locale)
			}
			if resolved.Input.Context != nil {
				view.Context = resolved.Input.Context.Text
			}
			for _, notice := range resolved.Approval {
				view.Notices = append(view.Notices, toolLocalText(notice, locale))
			}
			// The card says what is being approved: the call's content, else
			// its first approval notice, else its target, else its summary.
			input := resolved.Input
			switch {
			case input.Content != nil:
				view.Description, view.Code = input.Content.Text, input.Content.Code
			case len(view.Notices) > 0:
				view.Description = view.Notices[0]
				view.Code = input.Summary != nil && input.Summary.Code && input.Summary.Text == view.Description
			case input.Target != nil:
				view.Description = input.Target.Text
			case input.Summary != nil:
				view.Description, view.Code = input.Summary.Text, input.Summary.Code
			}
		}
	}
	if approval.Changes != nil {
		view.Description = ""
		for _, file := range approval.Changes.Files {
			kind := composer.Modified
			switch file.Kind {
			case protocol.ChangeKindAdded:
				kind = composer.Added
			case protocol.ChangeKindRenamed:
				kind = composer.Renamed
			case protocol.ChangeKindDeleted:
				kind = composer.Deleted
			}
			change := composer.Change{Path: file.Path, Kind: kind, Added: int(file.Additions), Removed: int(file.Deletions), Truncated: file.Truncated}
			if file.Destination != nil {
				change.Destination = *file.Destination
			}
			view.Changes = append(view.Changes, change)
		}
	}
	if strings.TrimSpace(view.Context) == strings.TrimSpace(view.Description) {
		view.Context = ""
	}
	// Display templates can repeat the content as an approval notice. Keep the
	// full content once, along with any distinct context and original reasons.
	shown := map[string]bool{
		"":                                  true,
		strings.TrimSpace(view.Title):       true,
		strings.TrimSpace(view.Context):     true,
		strings.TrimSpace(view.Description): true,
	}
	view.Notices = slices.DeleteFunc(view.Notices, func(notice string) bool {
		text := strings.TrimSpace(notice)
		if shown[text] {
			return true
		}
		shown[text] = true
		return false
	})
	return view
}

func toolLocalText(value protocol.ToolResultLocalText, locale string) string {
	if translated := value.Locales[i18n.Match(locale)]; translated != "" {
		return translated
	}
	return value.Label
}
