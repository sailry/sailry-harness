// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type ProjectVisibility string

const (
	ProjectVisibilityPrivate ProjectVisibility = "private"
	ProjectVisibilityShared  ProjectVisibility = "shared"
)

func (value ProjectVisibility) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ProjectVisibility) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ProjectVisibility(decoded) {
	case ProjectVisibilityPrivate, ProjectVisibilityShared:
		*value = ProjectVisibility(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}

type ProjectsCatalog struct {
	Projects  []Project  `json:"projects"`
	Worktrees []Worktree `json:"worktrees"`
}

func DefaultProjectsCatalog() ProjectsCatalog {
	return ProjectsCatalog{
		Projects:  []Project{},
		Worktrees: []Worktree{},
	}
}
func (value *ProjectsCatalog) UnmarshalJSON(data []byte) error {
	type plain ProjectsCatalog
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"projects", "worktrees"}, false); err != nil {
		return err
	}
	*value = ProjectsCatalog(decoded)
	return nil
}
func (value ProjectsCatalog) MarshalJSON() ([]byte, error) {
	type plain ProjectsCatalog
	encoded := plain(value)
	encoded.Projects = normalizeCollections(encoded.Projects)
	encoded.Worktrees = normalizeCollections(encoded.Worktrees)
	return json.Marshal(encoded)
}

type ProjectsAppearance struct {
	Icon  string `json:"icon"`
	Color string `json:"color"`
}

func DefaultProjectsAppearance() ProjectsAppearance {
	return ProjectsAppearance{
		Icon:  "folder",
		Color: "none",
	}
}
func (value *ProjectsAppearance) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *ProjectsAppearance) DecodeJSON(decoder *Decoder) error {
	type plain ProjectsAppearance
	decoded := plain{}
	if err := decoder.Object(&decoded, []string{"icon", "color"}, false); err != nil {
		return err
	}
	*value = ProjectsAppearance(decoded)
	return nil
}
func (value ProjectsAppearance) MarshalJSON() ([]byte, error) {
	type plain ProjectsAppearance
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ProjectsSource struct {
	Kind string
	Data any
}
type ProjectsSourceClonePayload struct {
	Url    string  `json:"url"`
	Branch *string `json:"branch"`
}

func DefaultProjectsSourceClonePayload() ProjectsSourceClonePayload {
	return ProjectsSourceClonePayload{
		Url:    "",
		Branch: nil,
	}
}
func (value *ProjectsSourceClonePayload) UnmarshalJSON(data []byte) error {
	type plain ProjectsSourceClonePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"url"}, false); err != nil {
		return err
	}
	*value = ProjectsSourceClonePayload(decoded)
	return nil
}
func (value ProjectsSourceClonePayload) MarshalJSON() ([]byte, error) {
	type plain ProjectsSourceClonePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var projectsSourceVariants = variantFactories{"local": nil,
	"clone": func() any { return new(ProjectsSourceClonePayload) }}

func (value ProjectsSource) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, projectsSourceVariants)
}
func (value *ProjectsSource) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", projectsSourceVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

// ProjectsDraft creates a project; omitted visibility is shared. Stored Project
// values retain their legacy private default.
type ProjectsDraft struct {
	Name       string             `json:"name"`
	Path       string             `json:"path"`
	Appearance ProjectsAppearance `json:"appearance"`
	Source     ProjectsSource     `json:"source"`
	Visibility ProjectVisibility  `json:"visibility"`
}

func DefaultProjectsDraft() ProjectsDraft {
	return ProjectsDraft{
		Name:       "",
		Path:       "",
		Appearance: DefaultProjectsAppearance(),
		Source:     ProjectsSource{},
		Visibility: ProjectVisibilityShared,
	}
}
func (value *ProjectsDraft) UnmarshalJSON(data []byte) error {
	type plain ProjectsDraft
	decoded := plain{}
	decoded.Visibility = ProjectVisibilityShared
	if err := DecodeObject(data, &decoded, []string{"name", "path", "appearance", "source"}, false); err != nil {
		return err
	}
	*value = ProjectsDraft(decoded)
	return nil
}
func (value ProjectsDraft) MarshalJSON() ([]byte, error) {
	type plain ProjectsDraft
	encoded := plain(value)
	if encoded.Visibility == "" {
		encoded.Visibility = ProjectVisibilityShared
	}
	return json.Marshal(encoded)
}
