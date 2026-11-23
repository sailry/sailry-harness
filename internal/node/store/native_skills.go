package store

import (
	"context"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// NativeSkillsLookup returns the compile-time built-in skills contributed by
// assembled native Node modules.
type NativeSkillsLookup func() []protocol.NativeSkill

type nativeSkillsContextKey struct{}

// WithNativeSkills attaches a per-Node native skills lookup to ctx for
// turn-reference admission and snapshot projection.
func WithNativeSkills(ctx context.Context, lookup NativeSkillsLookup) context.Context {
	if lookup == nil {
		return ctx
	}
	return context.WithValue(ctx, nativeSkillsContextKey{}, lookup)
}

func nativeSkills(ctx context.Context) []protocol.NativeSkill {
	lookup, _ := ctx.Value(nativeSkillsContextKey{}).(NativeSkillsLookup)
	if lookup == nil {
		return nil
	}
	return lookup()
}

func hasNativeSkill(ctx context.Context, pkg, name string) bool {
	for _, entry := range nativeSkills(ctx) {
		if entry.Package == pkg && entry.Skill.Name == name {
			return true
		}
	}
	return false
}
