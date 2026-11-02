import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/configuration_commands.dart';
import 'package:sailry_mobile/runtime/json.dart';
import 'package:sailry_mobile/runtime/session.dart';

const team = {
  'id': 'team-1',
  'revision': 4,
  'model': {'provider': 'provider-new', 'model': 'new'},
};
const selectedConfig = {
  'provider': 'provider-new',
  'credential': 'credential-new',
  'model': 'new',
  'effort': 'high',
  'mode': 'flagship',
  'permission': 'ask',
};

class Fixture {
  Fixture({this.uncertain = true}) {
    commands = ConfigurationCommands(
      command: (kind, data) async {
        sent.add((kind, data!));
        final response = apply(kind, data);
        if (uncertain) {
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

  final bool uncertain;
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
      if (kind == 'set_session_team') ...{
        'roles': {
          'execution': data['execution'],
          'team': data['team'],
          'models': data['models'],
        },
        if (data['team'] != null) 'config': selectedConfig,
      },
      if (kind == 'set_session_config') 'config': data['config'],
    },
  };

  Future<void> select() =>
      commands.select('team', team, session: () => session);
}

void main() {
  test('model execution preserves the lead configuration', () async {
    final fixture = Fixture(uncertain: false);
    final config = fixture.session['config'];
    await fixture.commands.select(
      'model',
      team,
      session: () => fixture.session,
    );
    expect(fixture.sent.map((entry) => entry.$1), ['set_session_team']);
    expect(fixture.sent.single.$2, {
      'session': 'session-1',
      'expected_revision': 7,
      'execution': 'model',
      'team': null,
      'models': <String, dynamic>{},
    });
    expect(fixture.session['config'], config);
  });

  test('recovered model execution keeps its original request', () async {
    final fixture = Fixture();
    await expectLater(
      fixture.commands.select('model', null, session: () => fixture.session),
      throwsA(isA<CommandFailure>()),
    );
    await fixture.commands.check();
    expect(fixture.recovered, ['request-set_session_team']);
    expect(fixture.sent, hasLength(1));
    expect(object(fixture.session['roles'])['execution'], 'model');
    expect(object(fixture.session['config'])['model'], 'old');
    expect(fixture.commands.pending, isFalse);
  });

  test('team selection accepts the effective Node configuration', () async {
    final fixture = Fixture(uncertain: false);
    await fixture.select();
    expect(fixture.sent.map((entry) => entry.$1), ['set_session_team']);
    expect(fixture.session['revision'], 8);
    expect(fixture.session['config'], selectedConfig);
  });

  test('recovered team selection sends no follow-up command', () async {
    final fixture = Fixture();
    await expectLater(fixture.select(), throwsA(isA<CommandFailure>()));
    expect(fixture.commands.pending, isTrue);
    expect(fixture.sent, hasLength(1));
    await fixture.commands.check();
    expect(fixture.recovered, ['request-set_session_team']);
    expect(fixture.sent.map((entry) => entry.$1), ['set_session_team']);
    expect(fixture.session['revision'], 8);
    expect(fixture.session['config'], selectedConfig);
    expect(fixture.commands.pending, isFalse);
    await fixture.commands.check();
    expect(fixture.sent, hasLength(1));
    expect(fixture.recovered, hasLength(1));
  });

  test('uncertain recovery retains the original request', () async {
    final fixture = Fixture()..failure = 'outcome_unknown';
    await expectLater(fixture.select(), throwsA(isA<CommandFailure>()));
    await expectLater(fixture.commands.check(), throwsA(isA<CommandFailure>()));
    expect(fixture.sent, hasLength(1));
    expect(fixture.commands.pending, isTrue);
    fixture.failure = null;
    await fixture.commands.check();
    expect(fixture.sent, hasLength(1));
    expect(fixture.recovered, [
      'request-set_session_team',
      'request-set_session_team',
    ]);
    expect(fixture.session['config'], selectedConfig);
  });

  test('rejected selection clears the pending request', () async {
    final fixture = Fixture()..failure = 'revision_conflict';
    await expectLater(fixture.select(), throwsA(isA<CommandFailure>()));
    await expectLater(fixture.commands.check(), throwsA(isA<CommandFailure>()));
    expect(fixture.commands.pending, isFalse);
    fixture.failure = null;
    await fixture.commands.check();
    expect(fixture.sent, hasLength(1));
    expect(object(fixture.session['config'])['model'], 'old');
  });

  test('inactive owners cannot recover an operation', () async {
    final fixture = Fixture();
    await expectLater(fixture.select(), throwsA(isA<CommandFailure>()));
    fixture.active = false;
    await fixture.commands.check();
    expect(fixture.recovered, isEmpty);
    expect(fixture.sent, hasLength(1));
  });
}
