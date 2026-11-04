import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/team.dart';

const reviewer = {
  'id': 'role-review',
  'name': 'Reviewer',
  'model': {'provider': 'p-own', 'model': 'own', 'effort': null},
};
const builder = {'id': 'role-build', 'name': 'Builder', 'model': null};

const frontend = {
  'id': 'team-front',
  'revision': 3,
  'key': 'frontend',
  'name': 'Frontend',
  'appearance': {'icon': 'code', 'color': 'orange'},
  'description': '',
  'model': {'provider': 'p-lead', 'model': 'lead', 'effort': null},
  'instructions': '',
  'members': ['role-review', 'role-build'],
};

Map<String, dynamic> session({
  String? execution,
  Map<String, dynamic>? team,
  Map<String, dynamic> models = const {},
}) => {
  'id': 'session-1',
  'revision': 7,
  'roles': {
    'execution': ?execution,
    'profiles': [reviewer, builder],
    'providers': [],
    'team': ?team,
    'models': models,
  },
};

String tr(String key) => '<$key>';

void main() {
  group('commands', () {
    test("captures selection and member model commands", () {
      for (final (caseName, run) in <(String, void Function(String))>[
        (
          "team selection sends the stored revision",
          (caseName) {
            expect(selectCommand('session-1', 7, frontend), {
              'session': 'session-1',
              'expected_revision': 7,
              'execution': 'team',
              'models': <String, dynamic>{},
              'team': {'id': 'team-front', 'revision': 3},
            }, reason: caseName);
          },
        ),
        (
          "Default team sends a null team",
          (caseName) {
            expect(selectCommand('session-1', 7, null), {
              'session': 'session-1',
              'expected_revision': 7,
              'execution': 'team',
              'models': <String, dynamic>{},
              'team': null,
            }, reason: caseName);
          },
        ),
        (
          "member choice sends the role and model",
          (caseName) {
            expect(
              memberCommand('session-1', 8, 'role-build', {
                'provider': 'p-two',
                'model': 'fast',
              }),
              {
                'session': 'session-1',
                'expected_revision': 8,
                'role': 'role-build',
                'model': {'provider': 'p-two', 'model': 'fast', 'effort': null},
              },
              reason: caseName,
            );
          },
        ),
        (
          "member reset sends a null model",
          (caseName) {
            expect(
              memberCommand('session-1', 8, 'role-build', null)['model'],
              null,
              reason: caseName,
            );
          },
        ),
      ]) {
        run(caseName);
      }
    });
  });

  group('SessionTeam', () {
    test("resolves current and captured member appearances", () {
      for (final (caseName, run) in <(String, void Function(String))>[
        (
          "current appearances preserve captured membership and models",
          (caseName) {
            final captured = session();
            final before = session();
            final state = SessionTeam(
              captured,
              [frontend],
              catalog: [
                {
                  ...builder,
                  'appearance': {'icon': 'tools', 'color': 'orange'},
                  'model': {'provider': 'new', 'model': 'new'},
                },
                {
                  ...reviewer,
                  'appearance': {'icon': 'science', 'color': 'green'},
                  'model': {'provider': 'new', 'model': 'new'},
                },
                {'id': 'added-later', 'appearance': frontend['appearance']},
              ],
            );
            expect(state.members, [reviewer, builder], reason: caseName);
            expect(state.appearance(reviewer), {
              'icon': 'science',
              'color': 'green',
            }, reason: caseName);
            expect(state.appearance(builder), {
              'icon': 'tools',
              'color': 'orange',
            }, reason: caseName);
            expect(state.model(reviewer), reviewer['model'], reason: caseName);
            expect(state.model(builder), isNull, reason: caseName);
            expect(captured, before, reason: caseName);
          },
        ),
        (
          "removed roles keep appearance despite reused names",
          (caseName) {
            final removed = {
              ...reviewer,
              'appearance': {'icon': 'code', 'color': 'blue'},
            };
            final state = SessionTeam(
              session(),
              [],
              catalog: [
                {
                  ...removed,
                  'id': 'replacement',
                  'appearance': {'icon': 'ai', 'color': 'cyan'},
                },
              ],
            );
            expect(
              state.appearance(removed),
              removed['appearance'],
              reason: caseName,
            );
            expect(
              SessionTeam(session(), []).appearance(removed),
              removed['appearance'],
              reason: caseName,
            );
          },
        ),
        (
          "current neutral appearance replaces captured color",
          (caseName) {
            final captured = {
              ...reviewer,
              'appearance': {'icon': 'code', 'color': 'blue'},
            };
            final state = SessionTeam(
              session(),
              [],
              catalog: [
                {
                  ...reviewer,
                  'appearance': {'icon': 'writing', 'color': 'none'},
                },
              ],
            );
            expect(state.appearance(captured), {
              'icon': 'writing',
              'color': 'none',
            }, reason: caseName);
          },
        ),
      ]) {
        run(caseName);
      }
    });

    test("resolves stored default and model execution rosters", () {
      for (final (caseName, run) in <(String, void Function(String))>[
        (
          "omitted execution preserves the Default team",
          (caseName) {
            final state = SessionTeam(session(), [frontend]);
            expect(state.execution, 'team', reason: caseName);
            expect(state.members, hasLength(2), reason: caseName);
          },
        ),
        (
          "model execution has no delegation roster",
          (caseName) {
            final state = SessionTeam(
              session(
                execution: 'model',
                team: {'id': 'team-front', 'revision': 3},
              ),
              [frontend],
            );
            expect(state.execution, 'model', reason: caseName);
            expect(state.team, isNull, reason: caseName);
            expect(state.members, isEmpty, reason: caseName);
          },
        ),
        (
          "Default team has no stored team",
          (caseName) {
            expect(
              SessionTeam(session(), [frontend]).team,
              null,
              reason: caseName,
            );
          },
        ),
        (
          "resolves the captured team from the catalog",
          (caseName) {
            final state = SessionTeam(
              session(team: {'id': 'team-front', 'revision': 3}),
              [frontend],
            );
            expect(state.team?['name'], 'Frontend', reason: caseName);
            expect(state.members.map((m) => m['id']), [
              'role-review',
              'role-build',
            ], reason: caseName);
          },
        ),
      ]) {
        run(caseName);
      }
    });

    test("resolves member models and effort overrides", () {
      for (final (caseName, run) in <(String, void Function(String))>[
        (
          "override wins over the role model",
          (caseName) {
            final state = SessionTeam(
              session(
                models: {
                  'role-review': {
                    'provider': 'p-x',
                    'model': 'x',
                    'effort': null,
                  },
                },
              ),
              const [],
            );
            expect(state.overridden('role-review'), isTrue, reason: caseName);
            expect(state.model(reviewer)?['model'], 'x', reason: caseName);
          },
        ),
        (
          "member without a model follows the lead",
          (caseName) {
            final state = SessionTeam(session(), const []);
            expect(state.overridden('role-build'), isFalse, reason: caseName);
            expect(state.model(builder), null, reason: caseName);
            expect(state.model(reviewer)?['model'], 'own', reason: caseName);
          },
        ),
        (
          "picker selections use the latest lead configuration",
          (caseName) {
            final initial = SessionTeam(
              {
                ...session(),
                'config': {'provider': 'p-old', 'model': 'old'},
              },
              [frontend],
            );
            final updated = SessionTeam(
              {
                ...session(team: {'id': 'team-front', 'revision': 3}),
                'config': {
                  'provider': 'p-lead',
                  'model': 'lead',
                  'mode': 'flagship',
                },
              },
              [frontend],
            );
            expect(
              initial.selection(builder)['model'],
              'old',
              reason: caseName,
            );
            expect(updated.selection(null), updated.config, reason: caseName);
            expect(
              updated.selection(builder)['model'],
              'lead',
              reason: caseName,
            );
            expect(
              updated.selection(reviewer)['model'],
              'own',
              reason: caseName,
            );
          },
        ),
        (
          "effort keeps the member model",
          (caseName) {
            final state = SessionTeam(session(), const []);
            expect(
              memberCommand(
                'session-1',
                7,
                'role-review',
                state.effort(reviewer, 'high'),
              )['model'],
              {'provider': 'p-own', 'model': 'own', 'effort': 'high'},
              reason: caseName,
            );
          },
        ),
        (
          "effort for a Lead follower fixes the Lead model",
          (caseName) {
            final state = SessionTeam({
              ...session(),
              'config': {
                'provider': 'p-lead',
                'credential': 'c',
                'model': 'lead',
              },
            }, const []);
            expect(
              memberCommand(
                'session-1',
                7,
                'role-build',
                state.effort(builder, 'low'),
              )['model'],
              {'provider': 'p-lead', 'model': 'lead', 'effort': 'low'},
              reason: caseName,
            );
          },
        ),
      ]) {
        run(caseName);
      }
    });
  });

  group('chipLabel', () {
    const config = {'model': 'lead-model'};

    test("labels model default captured and removed teams", () {
      for (final (caseName, run) in <(String, void Function(String))>[
        (
          "model execution shows the model",
          (caseName) {
            expect(
              chipLabel(
                SessionTeam(session(execution: 'model'), [frontend]),
                config,
                tr,
              ),
              'lead-model',
              reason: caseName,
            );
          },
        ),
        (
          "Default team shows the model",
          (caseName) {
            expect(
              chipLabel(SessionTeam(session(), [frontend]), config, tr),
              'lead-model',
              reason: caseName,
            );
          },
        ),
        (
          "other teams show the team name",
          (caseName) {
            final state = SessionTeam(
              session(team: {'id': 'team-front', 'revision': 3}),
              [frontend],
            );
            expect(chipLabel(state, config, tr), 'Frontend', reason: caseName);
          },
        ),
        (
          "a removed team keeps a generic name",
          (caseName) {
            final state = SessionTeam(
              session(team: {'id': 'team-gone', 'revision': 1}),
              [frontend],
            );
            expect(chipLabel(state, config, tr), '<team>', reason: caseName);
          },
        ),
        (
          "missing model shows the empty label",
          (caseName) {
            expect(
              chipLabel(null, const {}, tr),
              '<conversationNoModel>',
              reason: caseName,
            );
          },
        ),
      ]) {
        run(caseName);
      }
    });
  });
}
