package plugins

import (
	"context"
	"encoding/json"
	"errors"
	"strconv"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func (owner *Host) sdkModule() HostModule {
	module := HostModule{Name: "sailry/sdk"}
	add := func(name string, async bool, operation HostOperation) {
		if async {
			module.Exports = append(module.Exports, hostAsync(name, operation))
		} else {
			module.Exports = append(module.Exports, hostSync(name, operation))
		}
	}
	add("newId", false, func(context.Context, []json.RawMessage) (json.RawMessage, error) {
		return hostJSON(protocol.NewRequestID().String())
	})
	add("context", false, func(context.Context, []json.RawMessage) (json.RawMessage, error) {
		body, _ := hostJSON(owner.scope())
		value, err := decodeHostJSON(body)
		if err != nil {
			return nil, err
		}
		hostStringRevision(value, "package", "settings_revision")
		return hostJSON(value)
	})
	add("prepareRequest", false, func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		if len(arguments) == 0 {
			return nil, hostInvalid("plugin command is required")
		}
		command, err := decodeHostCommand(arguments[0])
		if err != nil {
			return nil, err
		}
		return owner.prepare(ctx, command)
	})
	add("prepareTransaction", false, func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		operations, err := hostValue[[]json.RawMessage](arguments, 0)
		if err != nil {
			return nil, err
		}
		return owner.prepareData(ctx, "plugin_transaction", map[string]any{"operations": operations})
	})
	add("prepareNotification", false, func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		content, err := hostValue[map[string]json.RawMessage](arguments, 0)
		if err != nil {
			return nil, err
		}
		return owner.prepareData(ctx, "publish_notification", map[string]any{"package": owner.scope().Package, "content": content})
	})
	add("completeRequest", true, func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		identity, err := hostValue[string](arguments, 0)
		if err != nil {
			return nil, err
		}
		return owner.complete(ctx, identity, true)
	})
	add("forgetRequest", false, func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		identity, err := hostValue[string](arguments, 0)
		if err != nil {
			return nil, err
		}
		return owner.forget(ctx, identity)
	})
	for _, name := range []string{"errorCode", "faultCode"} {
		add(name, false, func(_ context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
			message, err := hostValue[string](arguments, 0)
			if err != nil {
				return nil, err
			}
			if name == "errorCode" {
				return hostJSON(hostErrorCode(message))
			}
			return hostJSON(hostFaultCode(message))
		})
	}
	add("isValueConflict", false, func(_ context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		if len(arguments) == 0 {
			return nil, hostInvalid("plugin outcome is required")
		}
		outcome, err := decodeHostJSON(arguments[0])
		if err != nil {
			return nil, err
		}
		return hostJSON(hostAt(outcome, "isError") == true &&
			hostAt(outcome, "error", "code") == "revision_conflict" &&
			hostAt(outcome, "error", "details", "kind") == "plugin_value_conflict")
	})
	add("readSettings", true, func(ctx context.Context, _ []json.RawMessage) (json.RawMessage, error) {
		return owner.readData(ctx, "read_plugin_settings", map[string]any{"package": owner.scope().Package})
	})
	add("listModels", true, owner.readOperation("list_plugin_models", nil))
	add("resolveModel", false, resolveHostModel)
	add("getValue", true, owner.readOperation("read_plugin_value", namedArguments("key")))
	add("listKeys", true, func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		prefix, err := hostOptional(arguments, 0, "")
		if err != nil {
			return nil, err
		}
		after, err := hostOptional[*string](arguments, 1, nil)
		if err != nil {
			return nil, err
		}
		limit, err := hostOptional(arguments, 2, uint16(100))
		if err != nil {
			return nil, err
		}
		return owner.readData(ctx, "list_plugin_keys", map[string]any{"prefix": prefix, "after": after, "limit": limit})
	})
	add("setValue", false, owner.prepareOperation("write_plugin_value", namedArguments("key", "value", "expected_revision")))
	add("indexedValue", false, owner.prepareOperation("write_indexed_plugin_value", namedArguments("key", "value", "index", "expected_revision")))
	add("deleteValue", false, owner.prepareOperation("remove_plugin_value", namedArguments("key", "expected_revision")))
	add("searchValues", true, func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		query, err := hostValue[map[string]json.RawMessage](arguments, 0)
		if err != nil {
			return nil, err
		}
		if _, exists := query["mode"]; !exists && nativeProp(query, "query", "") != "" {
			query["mode"] = json.RawMessage(`"hybrid"`)
		}
		return owner.readData(ctx, "search_plugin_values", query)
	})
	add("requestHttp", false, owner.prepareOperation("request_plugin_http", firstArgument))
	add("requestProviderHttp", true, owner.readOperation("request_provider_http", namedArgumentsWithDefaults(
		map[string]any{"input": map[string]string{}}, "provider", "expected_revision", "request", "input",
	)))
	add("prepareProviderHttp", false, owner.prepareOperation("execute_provider_http", namedArgumentsWithDefaults(
		map[string]any{"input": map[string]string{}}, "provider", "expected_revision", "request", "input",
	)))
	add("publishContributions", false, func(_ context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		if owner.binding.Contributions.Publish == nil {
			return nil, hostUnavailable("plugin has no contribution surface")
		}
		if len(arguments) == 0 {
			return nil, hostInvalid("contribution states are required")
		}
		err := owner.binding.Contributions.Publish(arguments[0])
		return json.RawMessage("null"), err
	})
	add("closeContribution", false, func(_ context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		if owner.binding.Contributions.Close == nil {
			return nil, hostUnavailable("plugin has no contribution surface")
		}
		identity, err := hostValue[string](arguments, 0)
		if err != nil {
			return nil, err
		}
		err = owner.binding.Contributions.Close(identity)
		return json.RawMessage("null"), err
	})
	add("nextContributionEvent", true, func(ctx context.Context, _ []json.RawMessage) (json.RawMessage, error) {
		if owner.binding.Contributions.Next == nil {
			return nil, hostUnavailable("plugin has no contribution surface")
		}
		return owner.binding.Contributions.Next(ctx)
	})
	add("nextChange", true, owner.changes.next)
	owner.addSessionSDK(add)
	owner.addWorkspaceSDK(add)
	owner.addUsageSDK(add)
	for _, names := range []struct {
		name  string
		async bool
	}{
		{"readComputerPermissions", true}, {"requestComputerPermission", true},
		{"readActivityCatalog", true}, {"readActivityPreviews", true}, {"nextActivityHosts", true},
	} {
		add(names.name, names.async, owner.nativeOperation("sailry/sdk", names.name))
	}
	return module
}

type hostArguments func([]json.RawMessage) (any, error)

func firstArgument(arguments []json.RawMessage) (any, error) {
	if len(arguments) == 0 {
		return nil, hostInvalid("plugin host argument is required")
	}
	return decodeHostJSON(arguments[0])
}

func namedArguments(names ...string) hostArguments {
	return namedArgumentsWithDefaults(nil, names...)
}

func namedArgumentsWithDefaults(defaults map[string]any, names ...string) hostArguments {
	return func(arguments []json.RawMessage) (any, error) {
		values := map[string]any{}
		for name, value := range defaults {
			values[name] = value
		}
		for index, name := range names {
			if index >= len(arguments) {
				if _, exists := defaults[name]; exists {
					continue
				}
				return nil, hostInvalid("plugin host argument is required")
			}
			value, err := decodeHostJSON(arguments[index])
			if err != nil {
				return nil, err
			}
			values[name] = value
		}
		return values, nil
	}
}

func (owner *Host) readOperation(kind string, arguments hostArguments) HostOperation {
	return func(ctx context.Context, input []json.RawMessage) (json.RawMessage, error) {
		var data any
		if arguments != nil {
			var err error
			data, err = arguments(input)
			if err != nil {
				return nil, err
			}
		}
		return owner.readData(ctx, kind, data)
	}
}

func (owner *Host) prepareOperation(kind string, arguments hostArguments) HostOperation {
	return func(ctx context.Context, input []json.RawMessage) (json.RawMessage, error) {
		var data any
		if arguments != nil {
			var err error
			data, err = arguments(input)
			if err != nil {
				return nil, err
			}
		}
		return owner.prepareData(ctx, kind, data)
	}
}

func resolveHostModel(_ context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
	if len(arguments) < 3 {
		return nil, errors.New("configure")
	}
	settings, err := decodeHostJSON(arguments[0])
	if err != nil {
		return nil, err
	}
	catalog, err := decodeHostJSON(arguments[1])
	if err != nil {
		return nil, err
	}
	field, err := hostValue[string](arguments, 2)
	if err != nil {
		return nil, err
	}
	selected, _ := hostAt(settings, "values", field).(string)
	models, _ := hostAt(catalog, "models").([]any)
	for _, model := range models {
		if hostAt(model, "id") != selected || hostAt(model, "kind") != "provider" {
			continue
		}
		resolved := map[string]any{"model": selected, "kind": "provider"}
		effort, _ := hostAt(settings, "values", field+"_effort").(string)
		if effort != "" {
			efforts, _ := hostAt(model, "efforts").([]any)
			found := false
			for _, candidate := range efforts {
				key, _ := candidate.(string)
				if budget, ok := hostAt(candidate, "budget").(json.Number); ok {
					key = budget.String()
				}
				if key == effort {
					resolved["effort"], found = candidate, true
					break
				}
			}
			if !found {
				return nil, errors.New("configure")
			}
		}
		return hostJSON(resolved)
	}
	return nil, errors.New("configure")
}

func exactRevisionText(value uint64) string {
	return strconv.FormatUint(value, 10)
}
