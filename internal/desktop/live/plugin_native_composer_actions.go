package live

import (
	"context"
	"encoding/json"
	"maps"
	"slices"
	"strconv"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	nodeplugins "github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (state *pluginNativeState) setComposerOption(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
	cursor, err := pluginNativeArgument[string](arguments, 0)
	if err != nil {
		return nil, err
	}
	key, err := pluginNativeArgument[string](arguments, 1)
	if err != nil || len(arguments) < 3 {
		return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid composer option")
	}
	value := append(json.RawMessage{}, arguments[2]...)
	return state.update(ctx, func(view *ui.Context) (json.RawMessage, error) {
		info, available := state.packageInfo()
		if !available || info.Extension == nil || !slices.Contains(info.Extension.Actions, protocol.PluginActionControlConversation) {
			return nil, pluginNativeFault(protocol.ErrorCodePermissionDenied, "composer control is unavailable")
		}
		state.refreshComposer()
		state.composer.mu.Lock()
		current := strconv.FormatUint(state.composer.view.cursor, 10)
		state.composer.mu.Unlock()
		if cursor != current {
			return nil, pluginNativeFault(protocol.ErrorCodeRevisionConflict, "composer options changed")
		}
		source, comp, config, err := state.composerSource()
		if err != nil {
			return nil, err
		}
		session, existing := state.runtime.session(source.ID)
		if !state.runtime.controller.Connected() || existing && delegated(session) ||
			state.composerBusy(source.ID) {
			return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "composer is unavailable")
		}
		switch key {
		case "command":
			if err := state.setComposerCommand(comp, value); err != nil {
				return nil, err
			}
		case "mode":
			choices := state.composerModes()
			mode := ""
			if json.Unmarshal(value, &mode) == nil {
				if mode != "code" && mode != "plan" && mode != "flagship" {
					return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid composer mode")
				}
			} else {
				var selection struct {
					Package struct {
						Name             string          `json:"name"`
						Digest           string          `json:"digest"`
						SettingsRevision json.RawMessage `json:"settings_revision"`
					} `json:"package"`
					ID string `json:"id"`
				}
				var revision string
				if json.Unmarshal(value, &selection) != nil ||
					json.Unmarshal(selection.Package.SettingsRevision, &revision) != nil ||
					selection.Package.Name != state.scope.Package.Name || selection.Package.Digest != state.scope.Package.Digest ||
					revision != strconv.FormatUint(state.scope.Package.SettingsRevision, 10) {
					return nil, pluginNativeFault(protocol.ErrorCodePermissionDenied, "conversation mode belongs to another package")
				}
				mode = selection.Package.Name + "/" + selection.ID
			}
			selected := client.SelectMode(config, choices, mode)
			if selected == nil {
				return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "conversation mode is unavailable")
			}
			config = *selected
			comp.Mode = modeChoice(config.Mode)
			state.configureComposer(comp, config, session, existing)
		case "plugin":
			var request struct {
				Key   string          `json:"key"`
				Value json.RawMessage `json:"value"`
			}
			if json.Unmarshal(value, &request) != nil || request.Key == "" || len(request.Value) == 0 || info.Extension.ConversationOptions == nil {
				return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "plugin option requires a declared key and value")
			}
			var schema protocol.PluginSettingsSchema
			if json.Unmarshal(*info.Extension.ConversationOptions, &schema) != nil {
				return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "plugin options are unavailable")
			}
			field, declared := schema.Properties[request.Key]
			if !declared || field.Secret != nil {
				return nil, pluginNativeFault(protocol.ErrorCodePermissionDenied, "plugin option is unavailable")
			}
			options := nodeplugins.SettingDefaults(&schema)
			maps.Copy(options, config.Options[state.scope.Package.Name])
			options[request.Key] = request.Value
			schema.Properties = map[string]protocol.PluginSettingsField{request.Key: field}
			schema.Required = []string{request.Key}
			if err := nodeplugins.ValidatePublicSettings(schema, map[string]json.RawMessage{request.Key: request.Value}); err != nil {
				return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "plugin option violates its declaration")
			}
			config.Options = maps.Clone(config.Options)
			if config.Options == nil {
				config.Options = map[string]map[string]json.RawMessage{}
			}
			config.Options[state.scope.Package.Name] = options
			state.configureComposer(comp, config, session, existing)
		default:
			return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "unknown composer option")
		}
		view.Invalidate()
		state.refreshComposer()
		return state.composerValue(false)
	})
}

func (state *pluginNativeState) configureComposer(comp *composer.State, config protocol.SessionConfig, session protocol.Session, existing bool) {
	state.runtime.composerBases[comp] = config
	if existing {
		state.runtime.saveComposerConfig(session, config, nil)
	}
}

func (state *pluginNativeState) composerModes() []client.ConversationMode {
	modes, _ := state.runtime.conversationModes()
	return modes
}

func (state *pluginNativeState) setComposerCommand(comp *composer.State, body json.RawMessage) error {
	declared := func(name string) bool {
		if registry := state.composerRegistry(); registry != nil {
			return slices.ContainsFunc(registry.Commands(), func(entry plugins.ContributionEntry) bool {
				command := entry.Declaration.Command
				return entry.Key.Package == state.scope.Package && entry.State.Enabled && command != nil &&
					command.Name == name && command.Kind == "message"
			})
		}
		return false
	}
	prefix := ""
	if string(body) != "null" {
		var name string
		if json.Unmarshal(body, &name) != nil || !declared(name) {
			return pluginNativeFault(protocol.ErrorCodePermissionDenied, "composer command is unavailable")
		}
		prefix = "/" + name + " "
	}
	end := 0
	if current, arguments, found := pluginNativeLeadingCommand(comp.Draft); found {
		if !declared(current) {
			return pluginNativeFault(protocol.ErrorCodePermissionDenied, "draft command belongs to another provider")
		}
		end = utf8.RuneCountInString(comp.Draft[:len(comp.Draft)-len(arguments)])
	} else if prefix == "" {
		return nil
	}
	if !comp.ReplacePluginCommandPrefix(end, prefix) {
		return pluginNativeFault(protocol.ErrorCodeUnavailable, "composer is composing")
	}
	return nil
}

func (state *pluginNativeState) revealComposerEntry(_ context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
	target, err := pluginNativeArgument[struct {
		Turn         protocol.TurnID `json:"turn"`
		TurnSequence string          `json:"turn_sequence"`
		Entry        string          `json:"entry"`
	}](arguments, 0)
	sequence, parseErr := strconv.ParseUint(target.TurnSequence, 10, 64)
	if err != nil || parseErr != nil || sequence == 0 || target.Entry == "" {
		return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid composer entry target")
	}
	info, available := state.packageInfo()
	if !available || info.Extension == nil || !slices.Contains(info.Extension.Actions, protocol.PluginActionReadConversation) || state.scope.Session == nil {
		return nil, pluginNativeFault(protocol.ErrorCodePermissionDenied, "composer entry reveal is unavailable")
	}
	return state.effect(func(*ui.Context) {
		if !state.runtime.controller.Connected() {
			return
		}
		history, loaded := state.runtime.controller.Conversation(*state.scope.Session)
		if !loaded || state.runtime.App.Messages.Reveal == nil {
			return
		}
		session := state.scope.Session.String()
		cancel := state.runtime.App.Messages.Reveal(session, conversation.Match{
			Turn: -1, Target: target.Turn.String(), TurnSequence: sequence, Entry: target.Entry, Revision: history.Snapshot.Page.Revision,
		}, func(index int, found bool) {
			if found && state.native.Context.Err() == nil {
				state.runtime.App.Messages.List(session).ScrollTo(index, ui.Center)
			}
		})
		context.AfterFunc(state.native.Context, cancel)
	})
}
