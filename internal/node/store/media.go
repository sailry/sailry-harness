package store

import (
	"context"
	"database/sql"
	"slices"

	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// mediaKinds lists the purposes a model can be assigned to, in display order.
var mediaKinds = []protocol.MediaKind{protocol.MediaKindVision, protocol.MediaKindImage, protocol.MediaKindVideo, protocol.MediaKindSpeech}

// MediaAssignment is one purpose's binding as captured when a turn was admitted.
type MediaAssignment struct {
	Model string `json:"model"`
	// Provider is absent when the bound model could not run at admission. The
	// purpose then has no model; it never falls back to another one.
	Provider *protocol.ConversationProvider `json:"provider,omitempty"`
}

// MediaCapture holds the assigned purposes only. An absent purpose follows
// the conversation model.
type MediaCapture map[protocol.MediaKind]MediaAssignment

// ReadMediaSettings returns the saved bindings and names every bound purpose
// whose model cannot run now.
func ReadMediaSettings(ctx context.Context, db *sql.Conn) (protocol.MediaSettings, error) {
	saved, err := readBody[protocol.MediaSettings](ctx, db, "SELECT body FROM media_settings WHERE singleton=1")
	if err != nil || saved == nil {
		return protocol.DefaultMediaSettings(), err
	}
	capture, err := captureMedia(ctx, db, *saved)
	if err != nil {
		return protocol.DefaultMediaSettings(), err
	}
	saved.Unavailable = nil
	for _, kind := range mediaKinds {
		if assignment, bound := capture[kind]; bound && assignment.Provider == nil {
			saved.Unavailable = append(saved.Unavailable, kind)
		}
	}
	return *saved, nil
}

// CaptureMedia freezes the per-purpose assignments for one admitted turn.
func CaptureMedia(ctx context.Context, db *sql.Conn) (MediaCapture, error) {
	saved, err := readBody[protocol.MediaSettings](ctx, db, "SELECT body FROM media_settings WHERE singleton=1")
	if err != nil || saved == nil {
		return MediaCapture{}, err
	}
	return captureMedia(ctx, db, *saved)
}

func captureMedia(ctx context.Context, db *sql.Conn, settings protocol.MediaSettings) (MediaCapture, error) {
	capture := MediaCapture{}
	for kind, binding := range settings.Bindings {
		if binding == nil {
			continue
		}
		provider, _, err := mediaProvider(ctx, db, kind, *binding)
		if err != nil {
			return nil, err
		}
		capture[kind] = MediaAssignment{Model: binding.Model, Provider: provider}
	}
	return capture, nil
}

// mediaProvider captures the bound provider. present reports that the enabled
// provider still lists the model; the provider is returned only when that
// model can also run the purpose.
func mediaProvider(ctx context.Context, db *sql.Conn, kind protocol.MediaKind, binding protocol.MediaBinding) (captured *protocol.ConversationProvider, present bool, err error) {
	saved, err := readBody[providerConfiguration](ctx, db, "SELECT body FROM providers WHERE id=?1", binding.Provider.String())
	if err != nil {
		return nil, false, err
	}
	if saved == nil || !saved.Listed || saved.Provider == nil || !saved.Provider.Enabled {
		return nil, false, nil
	}
	provider, err := CaptureProvider(ctx, db, *saved.Provider)
	if err != nil {
		// A provider whose channel declaration no longer resolves cannot run.
		return nil, false, nil
	}
	for _, entry := range provider.Models {
		if entry.ID == binding.Model {
			if _, supported := model.MediaChannelFor(kind, provider, entry); supported {
				return &provider, true, nil
			}
			return nil, true, nil
		}
	}
	return nil, false, nil
}

// MediaConfig selects one assigned model outside any session.
func MediaConfig(provider protocol.ProviderID, name string) protocol.SessionConfig {
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model = provider, name
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionAsk
	return config
}

// ResolveMediaBinding binds a captured API-key provider to its current key.
func ResolveMediaBinding(ctx context.Context, db *sql.Conn, node protocol.NodeID, provider protocol.ConversationProvider, name string, session protocol.SessionID) (model.Binding, error) {
	var secret *protocol.Secret
	if provider.Credential != nil {
		var err error
		if secret, err = resolveProviderKey(ctx, db, node, *provider.Credential, provider.ID); err != nil {
			return model.Binding{}, err
		}
	}
	return model.Capture(provider, MediaConfig(provider.ID, name), secret, session)
}

// MediaModels lists, per purpose, the enabled models whose capability tags
// include it and whose channel has a driver for it.
func MediaModels(ctx context.Context, db *sql.Conn) ([]protocol.MediaCandidate, error) {
	listed, err := providerList(ctx, db)
	if err != nil {
		return nil, err
	}
	candidates := []protocol.MediaCandidate{}
	for _, provider := range listed {
		if !provider.Enabled {
			continue
		}
		provider, err := CaptureProvider(ctx, db, provider)
		if err != nil {
			continue
		}
		for _, entry := range provider.Models {
			kinds := []protocol.MediaKind{}
			for _, kind := range mediaKinds {
				if _, supported := model.MediaChannelFor(kind, provider, entry); supported {
					kinds = append(kinds, kind)
				}
			}
			if len(kinds) > 0 {
				candidates = append(candidates, protocol.MediaCandidate{Provider: provider.ID, ProviderName: provider.Name, Model: entry.ID, Kinds: kinds})
			}
		}
	}
	return candidates, nil
}

func saveMediaSettings(ctx context.Context, db *sql.Conn, input protocol.MediaSettings) (protocol.Output, []protocol.Event, error) {
	current, err := readBody[protocol.MediaSettings](ctx, db, "SELECT body FROM media_settings WHERE singleton=1")
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if current == nil {
		empty := protocol.DefaultMediaSettings()
		current = &empty
	}
	if current.Revision != input.Revision {
		return protocol.Output{}, nil, fault(protocol.ErrorCodeRevisionConflict, "media settings changed")
	}
	bindings := map[protocol.MediaKind]*protocol.MediaBinding{}
	input.Unavailable = nil
	for kind, binding := range input.Bindings {
		if binding == nil {
			continue
		}
		if !slices.Contains(mediaKinds, kind) {
			return protocol.Output{}, nil, invalid("media purpose is unknown")
		}
		bindings[kind] = binding
		// A binding that is kept as saved may already be unavailable; only a
		// new choice has to run.
		if previous := current.Bindings[kind]; previous != nil && *previous == *binding {
			continue
		}
		provider, present, err := mediaProvider(ctx, db, kind, *binding)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if !present {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeNotConfigured, "media model is unavailable")
		}
		if provider == nil {
			return protocol.Output{}, nil, invalid("model does not support this media purpose")
		}
	}
	input.Bindings = bindings
	if input.Revision, err = nextRevision(input.Revision); err != nil {
		return protocol.Output{}, nil, invalid("media settings revision exhausted")
	}
	body, err := encode(input)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO media_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", body); err != nil {
		return protocol.Output{}, nil, err
	}
	saved, err := ReadMediaSettings(ctx, db)
	return protocol.Output{Kind: "media_settings", Data: saved}, []protocol.Event{{Kind: "media_settings_changed", Data: saved}}, err
}
