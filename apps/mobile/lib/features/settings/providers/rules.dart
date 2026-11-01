import 'dart:convert';
import 'dart:math' as math;

import '../../../runtime/json.dart';

/// Pure provider and model edits. Every function copies its input and keeps
/// fields it does not own, so Desktop-only data survives a mobile save.
typedef Json = Map<String, dynamic>;

const _defaultContext = 200000;
const _defaultOutput = 16384;

/// Named reasoning efforts in display order.
const namedEfforts = [
  'none',
  'minimal',
  'low',
  'medium',
  'high',
  'xhigh',
  'max',
];

/// Capability flags editable on mobile, in display order.
const capabilityNames = ['vision', 'tools', 'reasoning'];

String modelLabel(Json model) {
  final name = text(model['display_name']).trim();
  return name.isEmpty ? text(model['id']) : name;
}

/// Host and port of an endpoint, or the raw value when it is not a URL.
String endpointHost(String endpoint) {
  final uri = Uri.tryParse(endpoint.trim());
  if (uri == null || uri.host.isEmpty) return endpoint.trim();
  return uri.hasPort ? '${uri.host}:${uri.port}' : uri.host;
}

/// Compact token count, such as 128K or 1M.
String tokenLabel(int tokens) {
  String compact(double value) => value == value.roundToDouble()
      ? value.toStringAsFixed(0)
      : value.toStringAsFixed(1);
  if (tokens >= 1000000) return '${compact(tokens / 1000000)}M';
  if (tokens >= 1000) return '${compact(tokens / 1000)}K';
  return '$tokens';
}

/// A positive token count, or null for invalid input.
int? parseTokens(String value) {
  final tokens = int.tryParse(value.trim());
  return tokens != null && tokens > 0 && tokens <= 0x7fffffff ? tokens : null;
}

/// Request body for `save_provider`, checked against the revision it edits.
Json saveRequest(Json provider, {String? secret}) {
  final revision = number(provider['revision']).toInt();
  return {
    'provider': provider,
    'expected_revision': revision,
    'secret': secret,
  };
}

/// A new API-key provider without models.
Json newProvider({
  required String id,
  required String name,
  required String api,
  required String endpoint,
}) => {
  'id': id,
  'revision': 0,
  'name': name,
  'api': api,
  'authentication': 'api_key',
  'endpoint': endpoint,
  'enabled': true,
  'models': <Json>[],
  'default_model': '',
  'credential': null,
};

bool usesKey(Json provider) {
  final authentication = text(provider['authentication']);
  return authentication.isEmpty || authentication == 'api_key';
}

List<Json> models(Json provider) => objects(provider['models']);

Json? findModel(Json provider, String id) =>
    models(provider).where((model) => model['id'] == id).firstOrNull;

/// Replaces the model list and keeps the default model inside it.
Json withModels(Json provider, List<Json> next) {
  final ids = next.map((model) => text(model['id'])).toList();
  final current = text(provider['default_model']);
  return {
    ...provider,
    'models': next,
    'default_model': ids.contains(current) ? current : ids.firstOrNull ?? '',
  };
}

/// Replaces one model; a changed ID moves the default with it.
Json replaceModel(Json provider, String id, Json model) {
  final next = withModels(provider, [
    for (final item in models(provider)) item['id'] == id ? model : item,
  ]);
  final renamed = text(model['id']);
  return provider['default_model'] == id && renamed != id
      ? {...next, 'default_model': renamed}
      : next;
}

Json removeModel(Json provider, String id) => withModels(provider, [
  for (final model in models(provider))
    if (model['id'] != id) model,
]);

/// A changed ID drops metadata reported for the previous model, like Desktop.
Json renameModel(Json model, String id) {
  if (model['id'] == id) return model;
  return {...model, 'id': id}
    ..remove('display_name')
    ..remove('capabilities');
}

/// Sets a capability and records it as explicit metadata, so Node catalog
/// completion keeps the choice.
Json setCapability(Json model, String name, bool value) => {
  ...model,
  name: value,
  'capabilities': {...object(model['capabilities']), name: value},
};

/// Resolves discovered metadata against an optional catalog reference.
/// Mirrors the protocol's discovery configuration; Node completes catalog
/// reasoning efforts when the model is saved.
Json configure(Json discovered, [Json? reference]) {
  final capabilities = discovered['capabilities'] is Map
      ? object(discovered['capabilities'])
      : null;
  final reported =
      _optional(discovered['output']) ?? _optional(reference?['output']);
  final context =
      _optional(discovered['context']) ??
      _optional(reference?['context']) ??
      math.max(_defaultContext, reported ?? 0);
  final output = reported ?? math.min(_defaultOutput, context);
  final efforts = [
    for (final effort in capabilities?['efforts'] as List? ?? const [])
      if (effort != 'default') effort as Object,
  ];
  final inputs = (reference?['inputs'] as List?)?.whereType<String>() ?? [];
  final outputs = (reference?['outputs'] as List?)?.whereType<String>() ?? [];
  final name = text(discovered['display_name']);
  return {
    'id': text(discovered['id']),
    if (name.isNotEmpty) 'display_name': name,
    'context': context,
    'output': output,
    'capabilities': ?capabilities,
    'vision':
        capabilities?['vision'] as bool? ??
        (inputs.isEmpty || inputs.contains('image')),
    'tools':
        capabilities?['tools'] as bool? ?? reference?['tools'] as bool? ?? true,
    'reasoning':
        capabilities?['reasoning'] as bool? ??
        reference?['reasoning'] as bool? ??
        false,
    'generates': [
      for (final kind in outputs)
        if (kind == 'image' || kind == 'video') kind,
    ],
    'efforts': efforts,
    'custom_efforts': false,
    'default_effort': initialEffort(
      efforts,
      capabilities?['default_effort'] ?? 'default',
    ),
  };
}

int? _optional(Object? value) => value is num ? value.toInt() : null;

/// Appends selected discoveries that the provider does not have yet.
Json mergeDiscovered(
  Json provider,
  List<Json> selected,
  Map<String, Json> references,
) {
  final existing = models(provider).map((model) => model['id']).toSet();
  return withModels(provider, [
    ...models(provider),
    for (final model in selected)
      if (existing.add(model['id'])) configure(model, references[model['id']]),
  ]);
}

/// Stable identity of an effort value, including token budgets.
String effortKey(Object? effort) =>
    effort is String ? effort : jsonEncode(effort);

/// Named efforts the provider API accepts, mirroring protocol validation.
List<String> apiEfforts(String api) => switch (api) {
  'azure_ai' => const [],
  'deepseek' => const ['none', 'high', 'max'],
  'anthropic' => const ['low', 'medium', 'high', 'xhigh', 'max'],
  'gemini' => const ['none', 'low', 'medium', 'high'],
  'chat_completions' ||
  'responses' ||
  'azure_open_ai' ||
  'open_code_go' ||
  'open_code_zen' ||
  'plugin' => namedEfforts,
  _ => const ['low', 'medium', 'high'],
};

/// Selectable efforts: the API's named choices, then any other saved choice.
List<Object> effortOptions(String api, Json model) {
  final options = <Object>[...apiEfforts(api)];
  final keys = options.map(effortKey).toSet();
  for (final effort in model['efforts'] as List? ?? const []) {
    if (effort != null && effort != 'default' && keys.add(effortKey(effort))) {
      options.add(effort as Object);
    }
  }
  return options;
}

/// The preferred choice when present, else the first explicit one.
Object initialEffort(List<Object> choices, Object? preferred) {
  final keys = choices.map(effortKey).toSet();
  if (preferred != null &&
      preferred != 'default' &&
      keys.contains(effortKey(preferred))) {
    return preferred;
  }
  return choices.where((effort) => effort != 'default').firstOrNull ??
      'default';
}

/// Adds or removes an effort, keeping option order and a valid default.
Json toggleEffort(Json model, Object effort, List<Object> options) {
  final selected = {
    for (final item in model['efforts'] as List? ?? const []) effortKey(item),
  };
  final key = effortKey(effort);
  if (!selected.remove(key)) selected.add(key);
  final efforts = [
    for (final option in options)
      if (selected.contains(effortKey(option))) option,
  ];
  return {
    ...model,
    'efforts': efforts,
    'default_effort': initialEffort(efforts, model['default_effort']),
    'custom_efforts': true,
  };
}

Json setDefaultEffort(Json model, Object effort) => {
  ...model,
  'default_effort': effort,
  'custom_efforts': true,
};

/// Localization key of a limits problem, or null when the limits are valid.
String? limitsProblem(Json model) {
  final context = number(model['context']).toInt();
  final output = number(model['output']).toInt();
  if (context <= 0 || output <= 0) return 'providerNumberInvalid';
  if (output > context) return 'providerLimitsInvalid';
  return null;
}
