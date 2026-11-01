import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/configuration_commands.dart';
import 'package:sailry_mobile/runtime/json.dart';
import 'package:sailry_mobile/runtime/session.dart';

const team = {
  'id': 'team-1',
  'revision': 4,
  'model': {'provider': 'provider-new', 'model': 'new'},
};
const providers = [
  {
    'id': 'provider-new',
    'credential': 'credential-new',
    'models': [
      {'id': 'new', 'default_effort': 'high'},
    ],
  },
];

class Fixture {
  Fixture({this.uncertain = 'set_session_team'}) {
    commands = ConfigurationCommands(
      command: (kind, data) async {
        sent.add((kind, data!));
        final response = apply(kind, data);
        if (kind == uncertain) {
          receipts['request-$kind'] = response;
          throw CommandFailure('outcome_unknown', request: 'request-$kind');
        }
        return response;
      },
      execute: (request) async {
        recovered.add(request);
        final error = failure;
        if (error != null) throw CommandFailure(error, request: request);
        return receipts[request]!;
      },
      accept: (response) => session = object(response['data']),
      active: () => active,
    );
  }

  String? uncertain;
  final sent = <(String, Map<String, dynamic>)>[];
  final recovered = <String>[];
  final receipts = <String, Map<String, dynamic>>{};
  late final ConfigurationCommands commands;
  String? failure;
  bool active = true;
  Map<String, dynamic> session = {
    'id': 'session-1',
    'revision': 7,
    'config': {
      'provider': 'provider-old',
      'model': 'old',
      'mode': 'flagship',
      'permission': 'ask',
    },
  };

  Map<String, dynamic> apply(String kind, Map<String, dynamic> data) => {
    'data': {
      ...session,
      'revision': (data['expected_revision'] as int) + 1,
      if (kind == 'set_session_team') 'roles': {'team': data['team']},
      if (kind == 'set_session_config') 'config': data['config'],
    },
  };

  Future<void> select() =>
      commands.select(team, session: () => session, providers: () => providers);
}

void main() {
  test('confirmed team selection applies the lead model', () async {
    final fixture = Fixture(uncertain: null);
    await fixture.select();
    expect(fixture.sent.map((entry) => entry.$1), [
      'set_session_team',
      'set_session_config',
    ]);
    expect(fixture.sent.last.$2['expected_revision'], 8);
    expect(fixture.session['revision'], 9);
    expect(fixture.session['config'], {
      'provider': 'provider-new',
      'credential': 'credential-new',
      'model': 'new',
      'effort': 'high',
      'mode': 'flagship',
      'permission': 'ask',
    });
  });

  test('recovered team selection advances exactly once', () async {
    final fixture = Fixture();
    await expectLater(fixture.select(), throwsA(isA<CommandFailure>()));
    expect(fixture.commands.pending, isTrue);
    expect(fixture.sent, hasLength(1));
    await fixture.commands.check();
    expect(fixture.recovered, ['request-set_session_team']);
    expect(fixture.sent.map((entry) => entry.$1), [
      'set_session_team',
      'set_session_config',
    ]);
    expect(fixture.sent.last.$2['expected_revision'], 8);
    expect(object(fixture.session['config'])['model'], 'new');
    expect(fixture.commands.pending, isFalse);
    await fixture.commands.check();
    expect(fixture.sent, hasLength(2));
    expect(fixture.recovered, hasLength(1));
  });

  test('uncertain recovery retains the team continuation', () async {
    final fixture = Fixture()..failure = 'outcome_unknown';
    await expectLater(fixture.select(), throwsA(isA<CommandFailure>()));
    await expectLater(fixture.commands.check(), throwsA(isA<CommandFailure>()));
    expect(fixture.sent, hasLength(1));
    expect(fixture.commands.pending, isTrue);
    fixture.failure = null;
    await fixture.commands.check();
    expect(fixture.sent, hasLength(2));
    expect(fixture.recovered, [
      'request-set_session_team',
      'request-set_session_team',
    ]);
    expect(object(fixture.session['config'])['model'], 'new');
  });

  test('rejected team selection discards the continuation', () async {
    final fixture = Fixture()..failure = 'revision_conflict';
    await expectLater(fixture.select(), throwsA(isA<CommandFailure>()));
    await expectLater(fixture.commands.check(), throwsA(isA<CommandFailure>()));
    expect(fixture.commands.pending, isFalse);
    fixture.failure = null;
    await fixture.commands.check();
    expect(fixture.sent, hasLength(1));
    expect(object(fixture.session['config'])['model'], 'old');
  });

  test('uncertain lead update keeps its own request', () async {
    final fixture = Fixture(uncertain: 'set_session_config');
    await expectLater(fixture.select(), throwsA(isA<CommandFailure>()));
    expect(fixture.sent, hasLength(2));
    expect(fixture.session['revision'], 8);
    await fixture.commands.check();
    expect(fixture.recovered, ['request-set_session_config']);
    expect(fixture.sent, hasLength(2));
    expect(object(fixture.session['config'])['model'], 'new');
    expect(fixture.commands.pending, isFalse);
  });

  test('recovery retains an uncertain follow-up request', () async {
    final fixture = Fixture();
    await expectLater(fixture.select(), throwsA(isA<CommandFailure>()));
    fixture.uncertain = 'set_session_config';
    await expectLater(fixture.commands.check(), throwsA(isA<CommandFailure>()));
    expect(fixture.commands.pending, isTrue);
    expect(fixture.session['revision'], 8);
    await fixture.commands.check();
    expect(fixture.recovered, [
      'request-set_session_team',
      'request-set_session_config',
    ]);
    expect(fixture.sent, hasLength(2));
    expect(object(fixture.session['config'])['model'], 'new');
    expect(fixture.commands.pending, isFalse);
  });

  test('inactive owners cannot continue a recovered operation', () async {
    final fixture = Fixture();
    await expectLater(fixture.select(), throwsA(isA<CommandFailure>()));
    fixture.active = false;
    await fixture.commands.check();
    expect(fixture.recovered, isEmpty);
    expect(fixture.sent, hasLength(1));
  });
}
