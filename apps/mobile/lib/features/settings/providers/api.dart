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
