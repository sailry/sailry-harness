import 'dart:async';

import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/modes.dart';
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

ConversationModes controllerFor(HostConnection owner) => ConversationModes(
  owner,
  project: (_, modes) => {
    'selected': 'client-selection',
    'choices': modes.map((mode) => mode.key).toList(),
  },
  select: (config, _, _) => config,
);

void main() {
  test('loads metadata and presents localized mode labels', () async {
    final owner = host({'first': package('first')});
    final modes = controllerFor(owner);
    addTearDown(owner.dispose);
    addTearDown(modes.dispose);
    await modes.settled;
    expect(modes.selected(config()), 'client-selection');
    final choice = modes.choices(config()).single;
    expect(choice.label(const Locale('zh')), '多任务');
    expect(
      choice.label(
        const Locale.fromSubtags(languageCode: 'zh', scriptCode: 'Hant'),
      ),
      '多工',
    );
    expect(choice.wire['package'], {
      'name': 'first',
      'digest': 'first-1',
      'settings_revision': 0,
    });
  });

  test('loading cannot present or commit a selection', () async {
    final reply = Completer<Map<String, dynamic>>();
    final info = package('first');
    final owner = host({'first': info}, read: (_, _) => reply.future);
    final modes = controllerFor(owner);
    addTearDown(owner.dispose);
    addTearDown(modes.dispose);
    final current = config();
    expect(modes.ready, false);
    expect(modes.selected(current), isNull);
    expect(modes.choices(current), isEmpty);
    expect(modes.select(current, 'code'), isNull);
    reply.complete({'data': info});
    await modes.settled;
    expect(modes.selected(current), 'client-selection');
    expect(modes.select(current, 'code'), same(current));
    owner.connected = false;
    owner.notifyListeners();
    expect(modes.select(current, 'code'), isNull);
  });

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
    final modes = controllerFor(owner);
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
    expect(modes.choices(config()), isEmpty);
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
    final modes = controllerFor(owner);
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

  test('read failures leave the current configuration available', () async {
    final owner = host({
      'first': package('first'),
    }, read: (_, _) async => throw const CommandFailure('not_configured'));
    final modes = controllerFor(owner);
    addTearDown(owner.dispose);
    addTearDown(modes.dispose);
    final current = config();
    await modes.settled;
    expect(modes.error, isA<CommandFailure>());
    expect(modes.selected(current), isNull);
    expect(modes.select(current, 'code'), isNull);
    expect(current, config());
  });
}
