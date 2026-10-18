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
