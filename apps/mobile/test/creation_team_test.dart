import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/creation_draft.dart';
import 'package:sailry_mobile/runtime/json.dart';
import 'package:sailry_mobile/runtime/session.dart';

import 'support/drafts.dart';

const features = [
  {
    'id': 'speed',
    'options': [
      {'id': 'fast'},
    ],
  },
];

const builder = {'id': 'builder', 'name': 'Builder', 'model': null};
const reviewer = {
  'id': 'reviewer',
  'name': 'Reviewer',
  'model': {'provider': 'a', 'model': 'a-first', 'effort': null},
};
const frontend = {
  'id': 'frontend',
  'revision': 3,
  'name': 'Frontend',
  'members': ['reviewer', 'builder'],
  'model': {'provider': 'b', 'model': 'b-second', 'effort': null},
};

class Fixture {
  Fixture() {
    host = HostConnection.test(
      id: 'host',
      label: 'Host',
      snapshot:
          snapshot(
              'a',
              providers: [
                for (final id in ['a', 'b'])
                  {
                    ...provider(id),
                    'api': 'responses',
                    'models': [
                      for (final model in objects(provider(id)['models']))
                        {
                          ...model,
                          'capabilities': {'features': features},
                        },
                    ],
                  },
              ],
            )
            ..['roles'] = [builder, reviewer]
            ..['teams'] = [frontend],
      connection: recovery,
      command: (kind, data) async {
        commands.add((kind, data!));
        if (kind == 'read_session') {
          expect(data, {'session': 'created'});
          return sessionReceipt(current);
        }
        expect(kind, 'start_session');
        current = {
          ...admitted(data),
          'roles': {
            'execution': data['execution'],
            'team': data['team'],
            'models': data['models'],
          },
          if (data['team'] != null && data['use_explicit_config'] != true)
            'config': {
              ...object(data['config']),
              ...object(frontend['model']),
              'credential': provider('b')['credential'],
              'effort': 'high',
            },
        };
        final response = startReceipt();
        if (uncertain) {
          recovery.result = response;
          throw const CommandFailure('outcome_unknown', request: 'original');
        }
        return response;
      },
    );
    draft = CreationDraft(host);
  }

  final recovery = RecoveryConnection();
  final commands = <(String, Map<String, dynamic>)>[];
  late final HostConnection host;
  late final CreationDraft draft;
  Map<String, dynamic> current = {};
  bool uncertain = false;
  Map<String, dynamic> get start => commands.first.$2;

  void dispose() {
    draft.dispose();
    host.dispose();
  }
}

void main() {
  test('default roster follows current catalog without creating a session', () {
    final fixture = Fixture();
    addTearDown(fixture.dispose);
    expect(fixture.draft.team.members, [builder, reviewer]);
    expect(fixture.draft.team.team, isNull);
    fixture.host.snapshot['roles'] = [reviewer];
    fixture.host.notifyListeners();
    expect(fixture.draft.team.members, [reviewer]);
    expect(fixture.commands, isEmpty);
  });

  test('team previews lead and ordered members without changing mode', () {
    final fixture = Fixture();
    addTearDown(fixture.dispose);
    for (final mode in ['code', 'flagship']) {
      fixture.draft.configure({...fixture.draft.config, 'mode': mode});
      fixture.draft.selectTeam(frontend);
      expect(fixture.draft.team.members, [reviewer, builder]);
      expect(fixture.draft.config['provider'], 'b');
      expect(fixture.draft.config['model'], 'b-second');
      expect(fixture.draft.config['effort'], 'high');
      expect(fixture.draft.config['mode'], mode);
      fixture.draft.selectTeam(null);
      expect(fixture.draft.team.members, [builder, reviewer]);
      expect(fixture.draft.config['model'], 'b-second');
    }
    expect(fixture.commands, isEmpty);
  });

  test(
    'team changes clear member overrides and reset restores inheritance',
    () {
      final fixture = Fixture();
      addTearDown(fixture.dispose);
      final draft = fixture.draft;
      draft.selectTeam(frontend);
      draft.configureMember('builder', object(reviewer['model']));
      expect(draft.team.overridden('builder'), isTrue);
      expect(draft.team.selection(builder)['model'], 'a-first');
      draft.configureMember('builder', null);
      expect(draft.team.overridden('builder'), isFalse);
      expect(draft.team.selection(builder)['model'], 'b-second');
      draft.configureMember('builder', object(reviewer['model']));
      draft.selectTeam(frontend);
      expect(draft.team.overridden('builder'), isTrue);
      draft.selectTeam(null);
      expect(draft.team.overridden('builder'), isFalse);
    },
  );

  test('first admission applies exact team and all member models', () async {
    final fixture = Fixture();
    addTearDown(fixture.dispose);
    final draft = fixture.draft;
    draft.selectTeam(frontend);
    draft.configureMember('builder', {
      'provider': 'a',
      'model': 'a-second',
      'effort': 'high',
      'credential': 'unused',
    });
    await draft.send(message: 'First message', attachments: ['image']);
    expect(fixture.commands.map((item) => item.$1), [
      'start_session',
      'read_session',
    ]);
    expect(fixture.start, {
      'project': null,
      'worktree': null,
      'config': draft.config,
      'message': {
        'text': 'First message',
        'attachments': ['image'],
      },
      'execution': 'team',
      'team': {'id': 'frontend', 'revision': 3},
      'models': {
        'builder': {'provider': 'a', 'model': 'a-second', 'effort': 'high'},
      },
    });
    expect(object(draft.created!['roles'])['team'], {
      'id': 'frontend',
      'revision': 3,
    });
    expect(draft.created!['config'], fixture.current['config']);
    await draft.send(message: 'First message');
    expect(fixture.commands, hasLength(2));
  });

  test('member model and effort retain features through admission', () async {
    for (final features in <Map<String, String>?>[
      {'speed': 'fast'},
      {},
      null,
    ]) {
      final fixture = Fixture();
      addTearDown(fixture.dispose);
      final draft = fixture.draft;
      final member = {
        ...reviewer,
        'model': {...object(reviewer['model']), 'features': ?features},
      };
      fixture.host.snapshot['roles'] = [builder, member];
      draft.configureMember(
        'reviewer',
        draft.team.choose(member, {
          'provider': 'b',
          'model': 'b-second',
          'effort': 'default',
        }),
      );
      draft.configureMember('reviewer', draft.team.effort(member, 'high'));
      await draft.send(message: 'First message');
      final models = object(fixture.start['models']);
      expect(models, {
        'reviewer': {
          'provider': 'b',
          'model': 'b-second',
          'effort': 'high',
          'features': ?features,
        },
      });
    }
  });

  test(
    'switching to an unsupported model admits an explicit empty choice',
    () async {
      final fixture = Fixture();
      addTearDown(fixture.dispose);
      final draft = fixture.draft;
      final member = {
        ...reviewer,
        'model': {
          ...object(reviewer['model']),
          'features': {'speed': 'fast'},
        },
      };
      fixture.host.snapshot['roles'] = [member];
      fixture.host.snapshot['providers'] = [provider('a'), provider('b')];
      draft.configureMember(
        'reviewer',
        draft.team.choose(member, {
          'provider': 'b',
          'model': 'b-second',
          'effort': 'default',
        }),
      );
      await draft.send(message: 'First message');
      expect(fixture.commands.first.$1, 'start_session');
      expect(fixture.start['models'], {
        'reviewer': {
          'provider': 'b',
          'model': 'b-second',
          'effort': 'default',
          'features': {},
        },
      });
    },
  );

  test(
    'first admission uses the explicitly chosen lead over the team',
    () async {
      final fixture = Fixture();
      addTearDown(fixture.dispose);
      final draft = fixture.draft;
      draft.selectTeam(frontend);
      draft.configure({
        ...draft.config,
        'provider': 'a',
        'credential': provider('a')['credential'],
        'model': 'a-second',
        'effort': 'high',
        'mode': 'flagship',
      });
      await draft.send(message: 'First message');
      expect(fixture.commands.map((item) => item.$1), [
        'start_session',
        'read_session',
      ]);
      expect(fixture.start['use_explicit_config'], isTrue);
      expect(fixture.start['config'], draft.config);
      expect(draft.created!['config'], draft.config);
    },
  );

  test(
    'pending admission retains the team and exact lead until recovered',
    () async {
      final fixture = Fixture()..uncertain = true;
      addTearDown(fixture.dispose);
      final draft = fixture.draft;
      draft.selectTeam(frontend);
      draft.configure({...draft.config, 'model': 'b-first'});
      await expectLater(
        draft.send(message: 'First message'),
        throwsA(isA<CommandFailure>()),
      );
      expect(draft.created, isNull);
      expect(draft.locationEditable, isFalse);
      expect(draft.pendingRequest, 'original');
      final config = Map.of(draft.config);
      draft.selectTeam(null);
      draft.configure({...draft.config, 'mode': 'plan'});
      expect(draft.team.team?['id'], 'frontend');
      expect(draft.config, config);
      await draft.send(message: '');
      expect(fixture.recovery.requests, ['original']);
      expect(fixture.commands.map((item) => item.$1), [
        'start_session',
        'read_session',
      ]);
      expect(draft.created?['id'], 'created');
      expect(object(draft.created!['config'])['model'], 'b-first');
      expect(draft.uncertain, isFalse);
    },
  );

  test(
    'rejected team admission keeps the composer available for correction',
    () async {
      final fixture = Fixture()..uncertain = true;
      addTearDown(fixture.dispose);
      final draft = fixture.draft;
      draft.selectTeam(frontend);
      await expectLater(
        draft.send(message: 'First message'),
        throwsA(isA<CommandFailure>()),
      );
      fixture.recovery.code = 'revision_conflict';
      await expectLater(
        draft.send(message: 'First message'),
        throwsA(isA<CommandFailure>()),
      );
      expect(draft.uncertain, isFalse);
      expect(draft.created, isNull);
      fixture.uncertain = false;
      draft.project = {'id': 'project-a'};
      draft.selectWorktree({'id': 'tree-a', 'project': 'project-a'});
      expect(draft.project?['id'], 'project-a');
      expect(draft.worktree?['id'], 'tree-a');
      draft.selectTeam(null);
      draft.configure({...draft.config, 'mode': 'flagship'});
      await draft.send(message: 'First message');
      expect(fixture.commands.map((item) => item.$1), [
        'start_session',
        'start_session',
        'read_session',
      ]);
      final corrected = fixture.commands[1].$2;
      expect(corrected['team'], isNull);
      expect(corrected['message'], fixture.start['message']);
      expect(object(draft.created!['config'])['mode'], 'flagship');
      expect(draft.created?['id'], 'created');
    },
  );
}
