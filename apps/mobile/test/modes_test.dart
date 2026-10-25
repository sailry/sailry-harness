import 'dart:async';

import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/modes.dart';
import 'package:sailry_mobile/runtime/json.dart';
import 'package:sailry_mobile/runtime/session.dart';

Map<String, dynamic> package(
  String name, {
  bool write = true,
  bool enabled = true,
  int revision = 1,
}) => {
  'summary': {
    'name': name,
    'revision': revision,
    'digest': '$name-$revision',
    'settings_revision': 0,
    'enabled': enabled,
  },
  'extension': {
    'api_version': 'v1',
    'scope': 'host',
    'conversation_options': {
      'type': 'object',
      'additionalProperties': false,
      'properties': {
        'mode': {
          'type': 'string',
          'default': '',
          'enum': ['', 'selected'],
        },
      },
    },
    'conversation_modes': [
      {
        'id': 'selected',
        'option': 'mode',
        'permissions': {'write': write},
        'instructions': 'Follow the selected workflow',
        'display': {
          'label': 'Multitask $revision',
          'locales': {'zh-CN': '多任务', 'zh-TW': '多工'},
        },
      },
    ],
  },
};

Map<String, dynamic> config() => {
  'provider': 'provider',
  'model': 'fixture',
  'effort': 'default',
  'credential': null,
  'mode': 'code',
  'permission': 'ask',
  'options': {
    'first': {
      'proactive': true,
      'retained': {'value': 42},
    },
    'second': {'other': false},
    'unrelated': {'choice': 'preserved'},
  },
};

HostConnection host(
  Map<String, Map<String, dynamic>> packages, {
  CommandHandler? read,
}) => HostConnection.test(
  id: 'host',
  label: 'Host',
  snapshot: {
    'plugins': packages.values.map((info) => info['summary']).toList(),
  },
  command:
      read ??
      (kind, data) async {
        expect(kind, 'read_plugin');
        return {'data': packages[data!['name']]};
      },
);

void main() {
  test('shows the declared selection and clears builtins', () async {
    final owner = host({'first': package('first')});
    final modes = ConversationModes(owner);
    addTearDown(owner.dispose);
    addTearDown(modes.dispose);
    await modes.settled;
    final original = config();
    expect(modes.selected(original), 'code');
    final active = modes.select(original, 'first/selected')!;
    expect(modes.selected(active), 'first/selected');
    expect(modes.choices(active).single.label(const Locale('zh')), '多任务');
    expect(
      modes
          .choices(active)
          .single
          .label(
            const Locale.fromSubtags(languageCode: 'zh', scriptCode: 'Hant'),
          ),
      '多工',
    );
    for (final builtin in ['code', 'plan']) {
      final changed = modes.select(active, builtin)!;
      expect(modes.selected(changed), builtin);
      expect(changed['mode'], builtin);
      expect(changed['permission'], 'ask');
      expect(object(object(changed['options'])['first'])['mode'], '');
      expect(object(object(changed['options'])['first'])['proactive'], true);
      expect(object(object(changed['options'])['first'])['retained'], {
        'value': 42,
      });
      expect(
        object(changed['options'])['unrelated'],
        object(original['options'])['unrelated'],
      );
    }
    expect(
      object(object(original['options'])['first']).containsKey('mode'),
      false,
    );
    expect(object(object(active['options'])['first'])['mode'], 'selected');
  });

  test('switches package ownership and maps readonly authority', () async {
    final owner = host({
      'first': package('first'),
      'second': package('second', write: false),
    });
    final modes = ConversationModes(owner);
    addTearDown(owner.dispose);
    addTearDown(modes.dispose);
    await modes.settled;
    final active = modes.select(config(), 'first/selected')!;
    final review = modes.select(active, 'second/selected')!;
    expect(modes.selected(review), 'second/selected');
    expect(review['mode'], 'plan');
    expect(review['permission'], 'ask');
    expect(object(object(review['options'])['first'])['mode'], '');
    expect(object(object(review['options'])['second'])['other'], false);
    final ambiguous = {
      ...review,
      'options': {
        ...object(review['options']),
        'first': {
          ...object(object(review['options'])['first']),
          'mode': 'selected',
        },
      },
    };
    expect(modes.selected(ambiguous), isNull);
    final build = modes.select(ambiguous, 'code')!;
    expect(modes.selected(build), 'code');
    expect(object(object(build['options'])['first'])['mode'], '');
    expect(object(object(build['options'])['second'])['mode'], '');
  });

  test('loading cannot mislabel or commit the current mode', () async {
    final reply = Completer<Map<String, dynamic>>();
    final info = package('first');
    final owner = host({'first': info}, read: (_, _) => reply.future);
    final modes = ConversationModes(owner);
    addTearDown(owner.dispose);
    addTearDown(modes.dispose);
    final current = config();
    (current['options'] as Map)['first'] = {'mode': 'selected'};
    expect(modes.ready, false);
    expect(modes.selected(current), isNull);
    expect(modes.choices(current), isEmpty);
    expect(modes.select(current, 'code'), isNull);
    reply.complete({'data': info});
    await modes.settled;
    expect(modes.selected(current), 'first/selected');
    expect(modes.select(current, 'code'), isNotNull);
  });

  test(
    'disabled current modes remain recognizable and can be cleared',
    () async {
      final owner = host({'first': package('first', enabled: false)});
      final modes = ConversationModes(owner);
      addTearDown(owner.dispose);
      addTearDown(modes.dispose);
      await modes.settled;
      final current = config();
      (current['options'] as Map)['first'] = {
        'mode': 'selected',
        'proactive': true,
      };
      expect(modes.selected(current), 'first/selected');
      expect(modes.choices(current).single.enabled, false);
      expect(modes.select(current, 'first/selected'), isNull);
      final build = modes.select(current, 'code')!;
      expect(modes.selected(build), 'code');
      expect(modes.choices(build), isEmpty);
      expect(object(object(build['options'])['first'])['proactive'], true);
    },
  );

  test('inventory changes invalidate only the affected metadata', () async {
    final packages = {'first': package('first'), 'second': package('second')};
    final reads = <String>[];
    final owner = host(
      packages,
      read: (kind, data) async {
        expect(kind, 'read_plugin');
        final name = data!['name'] as String;
        reads.add(name);
        return {'data': packages[name]};
      },
    );
    final modes = ConversationModes(owner);
    addTearDown(owner.dispose);
    addTearDown(modes.dispose);
    await modes.settled;
    expect(reads, ['first', 'second']);
    owner.notifyListeners();
    await modes.settled;
    expect(reads, ['first', 'second']);
    packages['first'] = package('first', revision: 2);
    owner.snapshot = {
      'plugins': packages.values.map((info) => info['summary']).toList(),
    };
    owner.notifyListeners();
    expect(modes.selected(config()), isNull);
    await modes.settled;
    expect(reads, ['first', 'second', 'first']);
    expect(
      modes.choices(config()).first.label(const Locale('en')),
      'Multitask 2',
    );
    owner.snapshot = {'plugins': []};
    owner.notifyListeners();
    await modes.settled;
    final retained = config();
    (retained['options'] as Map)['first'] = {
      'mode': 'selected',
      'proactive': true,
    };
    expect(modes.selected(retained), 'code');
    expect(object(modes.select(retained, 'code')!['options'])['first'], {
      'mode': 'selected',
      'proactive': true,
    });
  });

  test('late replies cannot replace current declarations', () async {
    final first = package('first');
    final reply = Completer<Map<String, dynamic>>();
    var reads = 0;
    final owner = host(
      {'first': first},
      read: (_, _) async {
        if (++reads == 1) return reply.future;
        return {'data': package('first', revision: 2)};
      },
    );
    final modes = ConversationModes(owner);
    addTearDown(owner.dispose);
    addTearDown(modes.dispose);
    final previous = modes.settled;
    owner.snapshot = {
      'plugins': [package('first', revision: 2)['summary']],
    };
    owner.notifyListeners();
    await modes.settled;
    reply.complete({'data': first});
    await previous;
    expect(
      modes.choices(config()).single.label(const Locale('en')),
      'Multitask 2',
    );
  });

  test(
    'read failures preserve configuration without a false selection',
    () async {
      final owner = host({
        'first': package('first'),
      }, read: (_, _) async => throw const CommandFailure('not_configured'));
      final modes = ConversationModes(owner);
      addTearDown(owner.dispose);
      addTearDown(modes.dispose);
      final current = config();
      await modes.settled;
      expect(modes.error, isA<CommandFailure>());
      expect(modes.selected(current), isNull);
      expect(modes.select(current, 'code'), isNull);
      expect(current, config());
    },
  );
}
