# Teams and roles

The Teams settings page keeps its accepted concurrency control, Teams and Roles
tabs, virtual Default team, and existing forms. `settings.TeamActions` connects
those controls to `live/teams_*.go`; preview retains its in-memory actions.
The forms use neutral `viewmodel.Team` and `viewmodel.Role` contracts, aliased by
preview. Their revisions belong to the opened draft and never advance when a
remote catalog refreshes.

The Node owns role and team catalogs, provider validation, built-in role rules,
positive child concurrency and frozen session rosters. The Default team is not
a stored record: it selects all current roles. The UI keeps its accepted 1–16
slider without reducing a larger stored Node budget during reads. Role and team
writes preserve fields outside the forms, including appearance, tool and skill
selection, turn budgets and model features. Team renames preserve their selection
keys. Creates retain their generated identities and complete prepared commands
while an outcome is uncertain.

Save closes a form only after success. Failed and conflicting saves retain the
draft. An uncertain save retains the exact request, locks editing and allows an
explicit Save retry. Deletion uses the displayed record's exact revision and
changes no catalog optimistically. Outcomes appear only as toasts.

`live/teams_composer.go` projects the virtual Default team, current catalog choices
and selected session roster into the existing model controls. Lead model state
stays separate from `composer.State.MemberModels`; inherited values are display
choices and only explicit member overrides become protocol models. Existing
session changes preserve their frozen roster when changing a member model.
Unchanged optional effort and feature fields retain their inheritance, including
when the lead changes. A feature edit records an explicit choice; a member model
change retains only its supported features. Explicitly selecting a team again
recaptures changed team or role revisions, while an unchanged selection keeps
member overrides. An empty captured roster remains empty until that selection.
`withComposerTeam` serializes configuration, team selection and member writes
before input admission. A failure keeps input and selection drafts; an uncertain
write retains its prepared command for an explicit admission retry before any
new configuration write. Resolving a fixed team lead reconciles unchanged
controls against the saved configuration. Draft admission uses the current
selected roster and ignores overrides for roles no longer in that roster.

Agent mentions carry exact role IDs and immutable revisions from the existing
session roster, or the current role catalog for an unsent draft. The existing
single-agent mention rule remains. Skill choices read `read_plugin_version` for
the installed, enabled package identities in the Node snapshot. Assistant sessions
use their captured package version. The controller never scans package files or
substitutes a newer digest, and selections retain `package:skill` identities.
Desktop-only package skills do not contribute execution choices. Loading and
executing skills belongs to the Node; Python readiness remains unavailable until
its runtime phase.

## Ownership and deviations

The accepted UI exposes skills in the mention catalog; the reference's command
catalog arrangement does not change that layout. The controller retains only
editor bases, pending request identity and immutable catalog metadata. Shared
Client subscriptions own conversation state, and Node storage owns business
records. Child activity and approvals use real, registered child session IDs;
see the conversation and panel contracts for those surfaces.

`TestTeamsProjection` groups editor revision preservation, inherited versus
explicit member models, immutable skill choices and actual child readonly
controls. The coordinator runs compilation and executable checks after stage
integration.
