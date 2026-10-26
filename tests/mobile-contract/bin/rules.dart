import 'dart:convert';
import 'package:sailry_bridge/api/rules.dart' as rules;

void check(bool condition, String message) {
  if (!condition) throw StateError(message);
}

void run() {
  final modes = [
    for (final (name, write) in [('first', true), ('second', false)])
      {
        'package': {'name': name, 'settings_revision': 0, 'digest': 'a' * 64},
        'declaration': {
          'id': 'review',
          'option': 'mode',
          'display': {'label': 'Review'},
          'permissions': {'write': write},
          'instructions': 'Inspect the requested changes',
        },
        'enabled': true,
      },
  ];
  final original = jsonEncode({
    'provider': '00000000-0000-4000-8000-000000000001',
    'model': 'fixture',
    'mode': 'code',
    'permission': 'ask',
    'credential': null,
    'options': {
      'first': {'proactive': true},
      'other': {'choice': 'preserved'},
    },
  });
  String select(String config, String key) =>
      rules.selectMode(config: config, modes: jsonEncode(modes), key: key)!;
  Map<String, dynamic> project(String config) =>
      jsonDecode(rules.projectModes(config: config, modes: jsonEncode(modes)));

  final active = select(original, 'first/review');
  check(project(active)['selected'] == 'first/review', 'mode owner over FFI');
  final review = select(active, 'second/review');
  final config = jsonDecode(review);
  check(config['mode'] == 'plan', 'read-only mode authority over FFI');
  check(config['permission'] == 'ask', 'mode preserves permission over FFI');
  check(config['options']['first']['mode'] == '', 'old mode marker cleared');
  check(
    config['options']['first']['proactive'] == true,
    'mode keeps other options',
  );
  check(
    config['options']['other']['choice'] == 'preserved',
    'unrelated owner kept',
  );
  modes[1]['enabled'] = false;
  check(
    project(review)['selected'] == 'second/review',
    'disabled selection recognized',
  );
  check(
    rules.selectMode(
          config: review,
          modes: jsonEncode(modes),
          key: 'second/review',
        ) ==
        null,
    'disabled mode cannot be selected',
  );
  final cleared = select(review, 'code');
  check(project(cleared)['selected'] == 'code', 'built-in selection over FFI');
  check(
    !(project(cleared)['choices'] as List).contains('second/review'),
    'disabled mode omitted after clearing',
  );
}
