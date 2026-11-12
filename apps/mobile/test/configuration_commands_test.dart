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
      resolve: (request) async {
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

  Future<void> select() => commands.select(team, session: () => session);
}

void main() {
  test(
    "accepts effective configuration after explicit team selection",
    () async {
      for (final (caseName, run) in <(String, Future<void> Function(String))>[
        (
          "Default team preserves the lead configuration",
          (caseName) async {
            final fixture = Fixture(uncertain: false);
            final config = fixture.session['config'];
            await fixture.commands.select(null, session: () => fixture.session);
            expect(fixture.sent.map((entry) => entry.$1), [
              'set_session_team',
            ], reason: caseName);
            expect(fixture.sent.single.$2, {
              'session': 'session-1',
              'expected_revision': 7,
              'execution': 'team',
              'team': null,
              'models': <String, dynamic>{},
            }, reason: caseName);
            expect(fixture.session['config'], config, reason: caseName);
          },
        ),
        (
          "team selection accepts the effective Node configuration",
          (caseName) async {
            final fixture = Fixture(uncertain: false);
            await fixture.select();
            expect(fixture.sent.map((entry) => entry.$1), [
              'set_session_team',
            ], reason: caseName);
            expect(fixture.session['revision'], 8, reason: caseName);
            expect(fixture.session['config'], selectedConfig, reason: caseName);
          },
        ),
      ]) {
        await run(caseName);
      }
    },
  );

  test("recovers only the original uncertain configuration request", () async {
    for (final (caseName, run) in <(String, Future<void> Function(String))>[
      (
        "recovered Default team keeps its original request",
        (caseName) async {
          final fixture = Fixture();
          await expectLater(
            fixture.commands.select(null, session: () => fixture.session),
            throwsA(isA<CommandFailure>()),
            reason: caseName,
          );
          await fixture.commands.check();
          expect(fixture.recovered, [
            'request-set_session_team',
          ], reason: caseName);
          expect(fixture.sent, hasLength(1), reason: caseName);
          expect(
            object(fixture.session['roles'])['execution'],
            'team',
            reason: caseName,
          );
          expect(
            object(fixture.session['config'])['model'],
            'old',
            reason: caseName,
          );
          expect(fixture.commands.pending, isFalse, reason: caseName);
        },
      ),
      (
        "recovered team selection sends no follow-up command",
        (caseName) async {
          final fixture = Fixture();
          await expectLater(
            fixture.select(),
            throwsA(isA<CommandFailure>()),
            reason: caseName,
          );
          expect(fixture.commands.pending, isTrue, reason: caseName);
          expect(fixture.sent, hasLength(1), reason: caseName);
          await fixture.commands.check();
          expect(fixture.recovered, [
            'request-set_session_team',
          ], reason: caseName);
          expect(fixture.sent.map((entry) => entry.$1), [
            'set_session_team',
          ], reason: caseName);
          expect(fixture.session['revision'], 8, reason: caseName);
          expect(fixture.session['config'], selectedConfig, reason: caseName);
          expect(fixture.commands.pending, isFalse, reason: caseName);
          await fixture.commands.check();
          expect(fixture.sent, hasLength(1), reason: caseName);
          expect(fixture.recovered, hasLength(1), reason: caseName);
        },
      ),
      (
        "uncertain recovery retains the original request",
        (caseName) async {
          final fixture = Fixture()..failure = 'outcome_unknown';
          await expectLater(
            fixture.select(),
            throwsA(isA<CommandFailure>()),
            reason: caseName,
          );
          await expectLater(
            fixture.commands.check(),
            throwsA(isA<CommandFailure>()),
            reason: caseName,
          );
          expect(fixture.sent, hasLength(1), reason: caseName);
          expect(fixture.commands.pending, isTrue, reason: caseName);
          fixture.failure = null;
          await fixture.commands.check();
          expect(fixture.sent, hasLength(1), reason: caseName);
          expect(fixture.recovered, [
            'request-set_session_team',
            'request-set_session_team',
          ], reason: caseName);
          expect(fixture.session['config'], selectedConfig, reason: caseName);
        },
      ),
    ]) {
      await run(caseName);
    }
  });

  test("blocks recovery after rejection or owner deactivation", () async {
    for (final (caseName, run) in <(String, Future<void> Function(String))>[
      (
        "rejected selection clears the pending request",
        (caseName) async {
          final fixture = Fixture()..failure = 'revision_conflict';
          await expectLater(
            fixture.select(),
            throwsA(isA<CommandFailure>()),
            reason: caseName,
          );
          await expectLater(
            fixture.commands.check(),
            throwsA(isA<CommandFailure>()),
            reason: caseName,
          );
          expect(fixture.commands.pending, isFalse, reason: caseName);
          fixture.failure = null;
          await fixture.commands.check();
          expect(fixture.sent, hasLength(1), reason: caseName);
          expect(
            object(fixture.session['config'])['model'],
            'old',
            reason: caseName,
          );
        },
      ),
      (
        "inactive owners cannot recover an operation",
        (caseName) async {
          final fixture = Fixture();
          await expectLater(
            fixture.select(),
            throwsA(isA<CommandFailure>()),
            reason: caseName,
          );
          fixture.active = false;
          await fixture.commands.check();
          expect(fixture.recovered, isEmpty, reason: caseName);
          expect(fixture.sent, hasLength(1), reason: caseName);
        },
      ),
    ]) {
      await run(caseName);
    }
  });
}
