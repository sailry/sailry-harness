import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/creation_draft.dart';
import 'package:sailry_mobile/runtime/json.dart';
import 'package:sailry_mobile/runtime/session.dart';

import 'support/drafts.dart';

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
      snapshot: snapshot('a', providers: [provider('a'), provider('b')])
        ..['roles'] = [builder, reviewer]
        ..['teams'] = [frontend],
      connection: recovery,
      command: (kind, data) async {
        commands.add((kind, data!));
        if (kind == 'create_session') {
          current = admitted(data);
        } else {
          current = {
            ...current,
            'revision': (data['expected_revision'] as int) + 1,
            if (kind == 'set_session_team') ...{
              'roles': {
                'execution': 'team',
                'team': data['team'],
                'models': data['models'],
              },
              if (data['team'] != null)
                'config': {
                  ...object(current['config']),
                  ...object(frontend['model']),
                  'credential': provider('b')['credential'],
                  'effort': 'high',
                },
            },
            if (kind == 'set_session_config') 'config': data['config'],
          };
        }
        final response = {'data': current};
        if (kind == uncertain) {
          recovery.result = response;
          throw CommandFailure('outcome_unknown', request: 'original-$kind');
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
  String? uncertain;

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
    await draft.send(hasInput: true);
    expect(fixture.commands.map((item) => item.$1), [
      'create_session',
      'set_session_team',
    ]);
    expect(fixture.commands.last.$2, {
      'session': 'created',
      'expected_revision': 1,
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
    await draft.send(hasInput: true);
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
      await draft.send(hasInput: true);
      final models = object(fixture.commands.last.$2['models']);
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

  test('chosen lead is restored before the first turn', () async {
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
    await draft.send(hasInput: true);
    expect(fixture.commands.map((item) => item.$1), [
      'create_session',
      'set_session_team',
      'set_session_config',
    ]);
    expect(fixture.commands.last.$2['expected_revision'], 2);
    expect(draft.created!['config'], draft.config);
  });

  for (final stage in ['set_session_team', 'set_session_config']) {
    test('$stage recovery retains the admitted session', () async {
      final fixture = Fixture()..uncertain = stage;
      addTearDown(fixture.dispose);
      final draft = fixture.draft;
      draft.selectTeam(frontend);
      if (stage == 'set_session_config') {
        draft.configure({...draft.config, 'model': 'b-first'});
      }
      await expectLater(
        draft.send(hasInput: true),
        throwsA(isA<CommandFailure>()),
      );
      expect(draft.created, isNull);
      expect(draft.locationEditable, isFalse);
      expect(draft.pendingRequest, 'original-$stage');
      final config = Map.of(draft.config);
      draft.selectTeam(null);
      draft.configure({...draft.config, 'mode': 'plan'});
      expect(draft.team.team?['id'], 'frontend');
      expect(draft.config, config);
      await draft.send(hasInput: false);
      expect(fixture.recovery.requests, ['original-$stage']);
      expect(
        fixture.commands.where((item) => item.$1 == 'create_session'),
        hasLength(1),
      );
      expect(fixture.commands.where((item) => item.$1 == stage), hasLength(1));
      expect(draft.created?['id'], 'created');
      expect(draft.uncertain, isFalse);
    });
  }

  test('recovered team revision admits the chosen lead once', () async {
    final fixture = Fixture()..uncertain = 'set_session_team';
    addTearDown(fixture.dispose);
    final draft = fixture.draft;
    draft.selectTeam(frontend);
    draft.configure({...draft.config, 'model': 'b-first'});
    await expectLater(
      draft.send(hasInput: true),
      throwsA(isA<CommandFailure>()),
    );
    await draft.send(hasInput: false);
    expect(fixture.recovery.requests, ['original-set_session_team']);
    expect(fixture.commands.map((item) => item.$1), [
      'create_session',
      'set_session_team',
      'set_session_config',
    ]);
    expect(fixture.commands.last.$2['expected_revision'], 2);
    expect(object(draft.created!['config'])['model'], 'b-first');
  });

  test(
    'rejected team admission allows correction without another session',
    () async {
      final fixture = Fixture()..uncertain = 'set_session_team';
      addTearDown(fixture.dispose);
      final draft = fixture.draft;
      draft.selectTeam(frontend);
      await expectLater(
        draft.send(hasInput: true),
        throwsA(isA<CommandFailure>()),
      );
      fixture.recovery.code = 'revision_conflict';
      await expectLater(
        draft.send(hasInput: true),
        throwsA(isA<CommandFailure>()),
      );
      expect(draft.uncertain, isFalse);
      fixture.uncertain = null;
      draft.project = {'id': 'project-a'};
      draft.selectWorktree({'id': 'tree-a', 'project': 'project-a'});
      expect(draft.project, isNull);
      expect(draft.worktree, isNull);
      draft.selectTeam(null);
      draft.configure({...draft.config, 'mode': 'flagship'});
      await draft.send(hasInput: true);
      expect(
        fixture.commands.where((item) => item.$1 == 'create_session'),
        hasLength(1),
      );
      expect(
        fixture.commands
            .where((item) => item.$1 == 'set_session_team')
            .last
            .$2['team'],
        isNull,
      );
      expect(object(draft.created!['config'])['mode'], 'flagship');
      expect(draft.created?['id'], 'created');
    },
  );
}
