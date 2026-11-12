import '../../../runtime/json.dart';

/// Known form choices; opaque APIs remain visible without being reinterpreted.
const values = [
  'chat_completions',
  'responses',
  'anthropic',
  'gemini',
  'deepseek',
  'open_code_go',
  'open_code_zen',
  'azure_open_ai',
  'azure_ai',
];

bool editable(String current) => values.contains(current);

List<String> choices(String current) => [
  ...values,
  if (!editable(current)) current,
];

/// APIs whose endpoint is fixed by the service.
String? fixedEndpoint(String api) => switch (api) {
  'open_code_go' => 'https://opencode.ai/zen/go/v1',
  'open_code_zen' => 'https://opencode.ai/zen/v1',
  _ => null,
};

/// Uses the Node's selected route, as Desktop does. A missing builtin route
/// needs the current directory before its endpoint can be edited.
bool? editableEndpoint(
  Map<String, dynamic> provider,
  List<Map<String, dynamic>> entries,
) {
  if (provider['preset'] == null) {
    return fixedEndpoint(text(provider['api'])) == null;
  }
  final reference = object(provider['preset']);
  final source = object(reference['source']);
  if (source['kind'] == 'partner' || source['kind'] == 'package') return true;
  if (source['kind'] != 'builtin') return false;
  final selected = reference['route'];
  for (final entry in entries) {
    final preset = object(entry['preset']);
    if (object(entry['source'])['kind'] != 'builtin' ||
        preset['id'] != reference['id']) {
      continue;
    }
    for (final route in objects(preset['routes'])) {
      if (_api(text(route['protocol'])) != provider['api']) continue;
      if (selected is Map &&
          [
            'protocol',
            'region',
            'product',
          ].any((field) => selected[field] != route[field])) {
        continue;
      }
      return text(route['endpoint']).isEmpty;
    }
  }
  return null;
}

String? _api(String protocol) => switch (protocol) {
  'openai-responses' => 'responses',
  'openai-chat-completions' => 'chat_completions',
  'opencode-go' => 'open_code_go',
  'opencode-zen' => 'open_code_zen',
  'azure-openai' => 'azure_open_ai',
  'azure-ai' => 'azure_ai',
  'anthropic' || 'gemini' || 'deepseek' || 'plugin' => protocol,
  _ => null,
};

/// Localization key of a known API; opaque APIs show their identifier.
String? labelKey(String api) => switch (api) {
  'chat_completions' => 'apiChatCompletions',
  'responses' => 'apiResponses',
  'anthropic' => 'apiAnthropic',
  'gemini' => 'apiGemini',
  'deepseek' => 'apiDeepSeek',
  'open_code_go' => 'settingsOpenCodeGo',
  'open_code_zen' => 'settingsOpenCodeZen',
  'azure_open_ai' => 'apiAzureOpenAi',
  'azure_ai' => 'apiAzureAi',
  'plugin' => 'apiPlugin',
  _ => null,
};
