package store

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type knowledgePackageKey struct{}

// WithKnowledgePackage retains a package after the trusted resource entry point
// checked its origin. Its Turn and Session belong to the execution Node, so the
// storage Node reads the captured package instead of resolving foreign IDs.
func WithKnowledgePackage(ctx context.Context, info protocol.PluginInfo) context.Context {
	return context.WithValue(ctx, knowledgePackageKey{}, info)
}

// KnowledgePackageFromContext reads only the package installed by the verified
// resource-call entry point, never a package declared in model arguments.
func KnowledgePackageFromContext(ctx context.Context) (protocol.PluginInfo, bool) {
	info, present := ctx.Value(knowledgePackageKey{}).(protocol.PluginInfo)
	return info, present
}

// WithKnowledgeScope checks the captured package identity before making it
// available to a local resource operation. Link caller and user verification
// belongs to the resource entry point and cannot be supplied in this payload.
func WithKnowledgeScope(ctx context.Context, scope *protocol.ResourceScope) (context.Context, error) {
	if scope == nil {
		return ctx, nil
	}
	if scope.Plugin == nil || scope.Origin == nil {
		return nil, knowledgeDenied("resource call requires its captured plugin origin")
	}
	info := *scope.Plugin
	if (protocol.PluginReference{Name: info.Summary.Name, Digest: info.Summary.Digest, SettingsRevision: info.Summary.SettingsRevision}) != scope.Origin.Package {
		return nil, knowledgeDenied("resource call package differs from its captured scope")
	}
	return WithKnowledgePackage(ctx, info), nil
}

// CheckKnowledgeScope checks a trusted runtime's declared knowledge action.
// Ordinary plugin requests keep using the main plugin authorization path.
func CheckKnowledgeScope(ctx context.Context, request protocol.Request) error {
	info, present := KnowledgePackageFromContext(ctx)
	if !present {
		return nil
	}
	if request.Plugin == nil || (protocol.PluginReference{Name: info.Summary.Name, Digest: info.Summary.Digest, SettingsRevision: info.Summary.SettingsRevision}) != request.Plugin.Package {
		return knowledgeDenied("resource call package differs from its captured scope")
	}
	action := protocol.PluginActionReadKnowledge
	if request.Command.Durable() {
		action = protocol.PluginActionWriteKnowledge
	}
	if !knowledgeDeclares(info, action) {
		return knowledgeDenied("plugin does not declare the knowledge action")
	}
	return nil
}

// projectKnowledge adds the actual storage identity and effective project
// visibility to a read. Legacy rows remain in place and are upgraded on writes.
func projectKnowledge(ctx context.Context, db *sql.Conn, stored *KnowledgeStored) error {
	node, err := nodeIdentity(ctx, db)
	if err != nil {
		return err
	}
	stored.Collection.Node = node
	bindKnowledgeSource(&stored.Collection.Definition, node)
	var owner protocol.User
	if stored.Capture.User == nil {
		owner, err = LocalUser(ctx, db)
	} else {
		owner, err = ResolveUser(ctx, db, *stored.Capture.User)
	}
	if err != nil {
		return err
	}
	stored.Collection.OwnerUser = owner.ID
	visibility, err := knowledgeProjectVisibility(ctx, db, *stored)
	if err != nil {
		return err
	}
	if visibility != nil {
		stored.Collection.Definition.Visibility = *visibility
	} else if stored.Collection.Definition.Visibility == "" {
		stored.Collection.Definition.Visibility = protocol.KnowledgeVisibilityPrivate
	}
	return nil
}

func bindKnowledgeSource(definition *protocol.KnowledgeDefinition, node protocol.NodeID) {
	switch value := definition.Source.Data.(type) {
	case *protocol.KnowledgeSourceWorktreePayload:
		copy := *value
		copy.Node = &node
		definition.Source.Data = &copy
	case protocol.KnowledgeSourceWorktreePayload:
		value.Node = &node
		definition.Source.Data = value
	case *protocol.KnowledgeSourceDirectoryPayload:
		copy := *value
		copy.Node = &node
		definition.Source.Data = &copy
	case protocol.KnowledgeSourceDirectoryPayload:
		value.Node = &node
		definition.Source.Data = value
	case *protocol.KnowledgeSourceFilePayload:
		copy := *value
		copy.Node = &node
		definition.Source.Data = &copy
	case protocol.KnowledgeSourceFilePayload:
		value.Node = &node
		definition.Source.Data = value
	case *protocol.KnowledgeSourceStoragePayload:
		copy := *value
		copy.Node = &node
		definition.Source.Data = &copy
	case protocol.KnowledgeSourceStoragePayload:
		value.Node = &node
		definition.Source.Data = value
	}
}

// CheckKnowledgeOutcome checks current collection access before a stored result
// can be disclosed again. A delete result has no remaining source to expose.
func CheckKnowledgeOutcome(ctx context.Context, db *sql.Conn, request protocol.Request, output protocol.Output) error {
	if output.Kind != "knowledge_collection" {
		return nil
	}
	var collection protocol.KnowledgeCollection
	switch value := output.Data.(type) {
	case protocol.KnowledgeCollection:
		collection = value
	case *protocol.KnowledgeCollection:
		if value == nil {
			return knowledgeDenied("knowledge collection is unavailable")
		}
		collection = *value
	default:
		return knowledgeDenied("knowledge collection is unavailable")
	}
	entries, err := AllowedKnowledge(ctx, db, request)
	if err != nil {
		return err
	}
	for _, stored := range entries {
		if stored.Collection.ID == collection.ID {
			return nil
		}
	}
	return knowledgeDenied("knowledge collection is unavailable")
}
