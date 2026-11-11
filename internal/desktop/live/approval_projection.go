package live

import (
	"slices"

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
				view.Title = approvalLocalText(*resolved.Label, locale)
			}
			if resolved.Input.Context != nil {
				view.Context = resolved.Input.Context.Text
			}
			for _, notice := range resolved.Approval {
				view.Notices = append(view.Notices, approvalLocalText(notice, locale))
			}
			// The card says what is being approved: the call's content, else
			// its first approval notice, else its target, else its summary.
			input := resolved.Input
			switch {
			case input.Content != nil:
				view.Description, view.Code = input.Content.Text, input.Content.Code
			case len(view.Notices) > 0:
				view.Description = view.Notices[0]
				view.Notices = slices.DeleteFunc(view.Notices, func(notice string) bool { return notice == view.Description })
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
	return view
}

func approvalLocalText(value protocol.ToolResultLocalText, locale string) string {
	if translated := value.Locales[i18n.Match(locale)]; translated != "" {
		return translated
	}
	return value.Label
}
