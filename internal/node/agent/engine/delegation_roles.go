package engine

import (
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type missingRoleRequirements struct {
	skills []string
	tools  []string
}

func (m missingRoleRequirements) reason() string {
	var reasons []string
	if len(m.skills) > 0 {
		reasons = append(reasons, "required skills are missing: "+strings.Join(m.skills, ", "))
	}
	if len(m.tools) > 0 {
		reasons = append(reasons, "required tools are unavailable: "+strings.Join(m.tools, ", "))
	}
	return strings.Join(reasons, "; ")
}

func (r *Runtime) missingRoleRequirements(x *execution, role protocol.RoleProfile) missingRoleRequirements {
	missing := missingRoleRequirements{skills: r.missingRoleSkills(x, role)}
	if role.Tools == nil || len(*role.Tools) == 0 {
		return missing
	}
	// Use the captured inheritance grant, not a parent's model-facing names or
	// package declarations. A child still rechecks its own scoped executors.
	tools := x.delegationTools.restrict(slices.Clone(x.tools))
	missing.tools = missingRoleTools(role, x.inv.Turn.Config.Mode, roleModelSupportsTools(x, role), tools, legacyToolAliases(tools))
	return missing
}

func missingRoleTools(role protocol.RoleProfile, mode protocol.WorkMode, modelTools bool, tools []Tool, legacy map[string]string) []string {
	var missing []string
	readOnly := role.ReadOnly || mode == protocol.WorkModePlan
	for _, selection := range *role.Tools {
		if !modelTools || !slices.ContainsFunc(tools, func(tool Tool) bool {
			return (!readOnly || tool.ReadOnly) && roleToolMatches(selection, tool, legacy)
		}) {
			missing = append(missing, roleToolLabel(selection))
		}
	}
	return missing
}

func roleToolLabel(selection protocol.PluginConversationTool) string {
	switch value := selection.Data.(type) {
	case *protocol.PluginConversationToolBuiltinPayload:
		return value.Name
	case *protocol.PluginConversationToolPackagePayload:
		return value.Package + "/" + value.Name
	default:
		return selection.Kind
	}
}

func roleModelSupportsTools(x *execution, role protocol.RoleProfile) bool {
	roster := x.inv.Turn.Roles
	selection := role.Model
	if override, ok := roster.Models[role.ID]; ok {
		selection = &override
	}
	if selection == nil {
		return modelSupportsTools(x.inv.Provider, x.inv.Turn.Config.Model)
	}
	for i := range roster.Providers {
		if roster.Providers[i].ID == selection.Provider {
			return modelSupportsTools(&roster.Providers[i], selection.Model)
		}
	}
	return false
}

func modelSupportsTools(provider *protocol.ConversationProvider, name string) bool {
	return provider != nil && slices.ContainsFunc(provider.Models, func(model protocol.ConversationModel) bool {
		return model.ID == name && model.Tools
	})
}
