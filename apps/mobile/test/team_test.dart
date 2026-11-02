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
    test('team selection sends the stored revision', () {
      expect(selectCommand('session-1', 7, frontend), {
        'session': 'session-1',
        'expected_revision': 7,
        'execution': 'team',
        'models': <String, dynamic>{},
        'team': {'id': 'team-front', 'revision': 3},
      });
    });

    test('Default team sends a null team', () {
      expect(selectCommand('session-1', 7, null), {
        'session': 'session-1',
        'expected_revision': 7,
        'execution': 'team',
        'models': <String, dynamic>{},
        'team': null,
      });
    });

    test('member choice sends the role and model', () {
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
      );
    });

    test('member reset sends a null model', () {
      expect(memberCommand('session-1', 8, 'role-build', null)['model'], null);
    });
  });

  group('SessionTeam', () {
    test('omitted execution preserves the Default team', () {
      final state = SessionTeam(session(), [frontend]);
      expect(state.execution, 'team');
      expect(state.members, hasLength(2));
    });

    test('model execution has no delegation roster', () {
      final state = SessionTeam(
        session(execution: 'model', team: {'id': 'team-front', 'revision': 3}),
        [frontend],
      );
      expect(state.execution, 'model');
      expect(state.team, isNull);
      expect(state.members, isEmpty);
    });

    test('Default team has no stored team', () {
      expect(SessionTeam(session(), [frontend]).team, null);
    });

    test('resolves the captured team from the catalog', () {
      final state = SessionTeam(
        session(team: {'id': 'team-front', 'revision': 3}),
        [frontend],
      );
      expect(state.team?['name'], 'Frontend');
      expect(state.members.map((m) => m['id']), ['role-review', 'role-build']);
    });

    test('override wins over the role model', () {
      final state = SessionTeam(
        session(
          models: {
            'role-review': {'provider': 'p-x', 'model': 'x', 'effort': null},
          },
        ),
        const [],
      );
      expect(state.overridden('role-review'), isTrue);
      expect(state.model(reviewer)?['model'], 'x');
    });

    test('member without a model follows the lead', () {
      final state = SessionTeam(session(), const []);
      expect(state.overridden('role-build'), isFalse);
      expect(state.model(builder), null);
      expect(state.model(reviewer)?['model'], 'own');
    });

    test('picker selections use the latest lead configuration', () {
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
          'config': {'provider': 'p-lead', 'model': 'lead', 'mode': 'flagship'},
        },
        [frontend],
      );
      expect(initial.selection(builder)['model'], 'old');
      expect(updated.selection(null), updated.config);
      expect(updated.selection(builder)['model'], 'lead');
      expect(updated.selection(reviewer)['model'], 'own');
    });

    test('effort keeps the member model', () {
      final state = SessionTeam(session(), const []);
      expect(
        memberCommand(
          'session-1',
          7,
          'role-review',
          state.effort(reviewer, 'high'),
        )['model'],
        {'provider': 'p-own', 'model': 'own', 'effort': 'high'},
      );
    });

    test('effort for a Lead follower fixes the Lead model', () {
      final state = SessionTeam({
        ...session(),
        'config': {'provider': 'p-lead', 'credential': 'c', 'model': 'lead'},
      }, const []);
      expect(
        memberCommand(
          'session-1',
          7,
          'role-build',
          state.effort(builder, 'low'),
        )['model'],
        {'provider': 'p-lead', 'model': 'lead', 'effort': 'low'},
      );
    });
  });

  group('chipLabel', () {
    const config = {'model': 'lead-model'};

    test('model execution shows the model', () {
      expect(
        chipLabel(
          SessionTeam(session(execution: 'model'), [frontend]),
          config,
          tr,
        ),
        'lead-model',
      );
    });

    test('Default team shows the model', () {
      expect(
        chipLabel(SessionTeam(session(), [frontend]), config, tr),
        'lead-model',
      );
    });

    test('other teams show the team name', () {
      final state = SessionTeam(
        session(team: {'id': 'team-front', 'revision': 3}),
        [frontend],
      );
      expect(chipLabel(state, config, tr), 'Frontend');
    });

    test('a removed team keeps a generic name', () {
      final state = SessionTeam(
        session(team: {'id': 'team-gone', 'revision': 1}),
        [frontend],
      );
      expect(chipLabel(state, config, tr), '<team>');
    });

    test('missing model shows the empty label', () {
      expect(chipLabel(null, const {}, tr), '<conversationNoModel>');
    });
  });
}
