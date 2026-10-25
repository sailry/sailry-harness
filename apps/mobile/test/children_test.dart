import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/children.dart';
import 'package:sailry_mobile/features/conversations/live/presentation.dart';

void main() {
  Map<String, dynamic> child(int sequence, String status) => {
    'run': {'session': 'child', 'sequence': sequence, 'status': status},
  };

  test('guidance follows the active then newest claimed run', () {
    final completed = child(2, 'completed');
    final running = child(4, 'running');
    final queued = child(5, 'queued');
    final children = [completed, running, queued];
    final before = jsonEncode(children);
    expect(latestChild(children), same(running));
    expect(latestChild([queued, running, completed]), same(running));
    expect(jsonEncode(children), before);
    final continued = child(4, 'continued');
    expect(latestChild([completed, continued, queued]), same(continued));
    final guided = child(5, 'running');
    expect(latestChild([completed, continued, guided]), same(guided));
    final stopped = child(6, 'cancelled');
    expect(latestChild([completed, continued, stopped]), same(stopped));
    expect(latestChild([child(5, 'queued'), child(6, 'queued')]), {
      'run': {'session': 'child', 'sequence': 6, 'status': 'queued'},
    });
    expect(latestChild([]), isNull);
    expect(
      runLabel('continued', translate: (key) => key),
      'conversationContinued',
    );
  });

  group('child navigation', () {
    final parent = {'id': 'parent', 'project': 'project', 'worktree': 'main'};
    final task = {
      'origin': {'session': 'parent', 'role': 'review'},
      'run': {'session': 'child', 'worktree': 'linked'},
    };
    final session = {
      'id': 'child',
      'project': 'project',
      'worktree': 'linked',
      'delegation': {'session': 'parent', 'role': 'review'},
    };
    final tree = {'id': 'linked', 'project': 'project'};
    Map<String, dynamic> snapshot({
      Map<String, dynamic>? selected,
      List<Map<String, dynamic>>? trees,
    }) => {
      'sessions': [selected ?? session],
      'worktrees': trees ?? [tree],
    };

    test('opens the captured registered worktree', () {
      final state = snapshot();
      final before = jsonEncode(state);
      expect(childSession(state, parent, task), session);
      expect(jsonEncode(state), before);
    });

    test('keeps project and delegation ownership', () {
      for (final changed in [
        {...session, 'project': 'other'},
        {...session, 'worktree': 'main'},
        {...session, 'delegation': null},
        {
          ...session,
          'delegation': {'session': 'other', 'role': 'review'},
        },
        {
          ...session,
          'delegation': {'session': 'parent', 'role': 'other'},
        },
      ]) {
        expect(childSession(snapshot(selected: changed), parent, task), isNull);
      }
      expect(childSession(snapshot(trees: []), parent, task), isNull);
      expect(
        childSession(
          snapshot(
            trees: [
              {...tree, 'project': 'other'},
            ],
          ),
          parent,
          task,
        ),
        isNull,
      );
    });

    test('unassigned children retain their private worktree', () {
      final unassigned = {...parent, 'project': null};
      final selected = {...session, 'project': null};
      final state = snapshot(
        selected: selected,
        trees: [
          {...tree, 'project': null},
        ],
      );
      expect(childSession(state, unassigned, task), selected);
    });
  });
}
