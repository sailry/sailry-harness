import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/attachments.dart';
import 'package:sailry_mobile/features/conversations/live/creation_draft.dart';
import 'package:sailry_mobile/features/conversations/live/turn_draft.dart';
import 'package:sailry_mobile/runtime/session.dart';

import 'support/drafts.dart';

PickedAttachment attachment(String id) =>
    (attachment: {'id': id}, preview: null);

void main() {
  for (final (code, rejected) in const [
    ('revision_conflict', true),
    ('invalid_request', true),
    ('conflict', true),
    ('not_found', true),
    ('not_configured', true),
    ('outcome_unknown', false),
    ('unavailable', false),
    ('permission_denied', false),
    ('busy', false),
  ]) {
    group(code, () {
      test('creation permits correction only after rejection', () async {
        final connection = RecoveryConnection()..code = code;
        var submissions = 0;
        final host = HostConnection.test(
          id: 'host',
          label: 'Host',
          snapshot: snapshot('a'),
          connection: connection,
          command: (_, _) async {
            submissions++;
            throw const CommandFailure('outcome_unknown', request: 'original');
          },
        );
        final draft = CreationDraft(host);
        addTearDown(draft.dispose);
        addTearDown(host.dispose);
        await expectLater(
          draft.send(hasInput: true),
          throwsA(isA<CommandFailure>()),
        );
        expect(draft.uncertain, isTrue);
        expect(draft.pendingRequest, 'original');
        await expectLater(
          draft.send(hasInput: true),
          throwsA(isA<CommandFailure>()),
        );
        expect(connection.requests, ['original']);
        expect(draft.uncertain, !rejected);
        expect(draft.pendingRequest, rejected ? isNull : 'original');
        await expectLater(
          draft.send(hasInput: true),
          throwsA(isA<CommandFailure>()),
        );
        expect(submissions, rejected ? 2 : 1);
        expect(
          connection.requests,
          rejected ? ['original'] : ['original', 'original'],
        );
      });

      test('turn input survives unresolved admission', () async {
        final connection = RecoveryConnection()..code = code;
        var submissions = 0;
        final host = HostConnection.test(
          id: 'host',
          label: 'Host',
          connection: connection,
          command: (_, _) async {
            submissions++;
            throw const CommandFailure('outcome_unknown', request: 'original');
          },
        );
        final file = attachment('file');
        final draft = TurnDraft(
          host: host,
          session: 'session',
          text: 'Do this once',
          attachments: [file],
        );
        addTearDown(draft.dispose);
        addTearDown(host.dispose);
        await expectLater(draft.send(7), throwsA(isA<CommandFailure>()));
        await expectLater(draft.check(), throwsA(isA<CommandFailure>()));
        expect(connection.requests, ['original']);
        expect(draft.input.text, 'Do this once');
        expect(draft.attachments, [file]);
        expect(draft.pending == null, rejected);
        if (rejected) {
          await expectLater(draft.send(8), throwsA(isA<CommandFailure>()));
          expect(submissions, 2);
        } else {
          await draft.send(8);
          expect(submissions, 1);
          await expectLater(draft.check(), throwsA(isA<CommandFailure>()));
          expect(connection.requests, ['original', 'original']);
        }
      });
    });
  }

  test('creation reconciliation uses the original Node result', () async {
    final connection = RecoveryConnection()
      ..result = {
        'data': {'id': 'created', 'worktree': 'node-workspace'},
      };
    var creates = 0;
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      snapshot: snapshot('a'),
      connection: connection,
      command: (_, _) async {
        creates++;
        throw const CommandFailure('outcome_unknown', request: 'original');
      },
    );
    final draft = CreationDraft(host);
    addTearDown(draft.dispose);
    addTearDown(host.dispose);
    await expectLater(
      draft.send(hasInput: true),
      throwsA(isA<CommandFailure>()),
    );
    host.snapshot = {};
    host.notifyListeners();
    expect(draft.project!['id'], 'project-a');
    await draft.send(hasInput: false);
    expect(creates, 1);
    expect(connection.requests, ['original']);
    expect(draft.created!['worktree'], 'node-workspace');
  });

  test('admission preserves newly typed text and attachments', () async {
    final reply = Completer<Map<String, dynamic>>();
    Map<String, dynamic>? requested;
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      command: (kind, data) {
        expect(kind, 'submit_turn');
        requested = data;
        return reply.future;
      },
    );
    final original = attachment('original');
    final newer = attachment('newer');
    final draft = TurnDraft(
      host: host,
      session: 'session',
      text: 'Original',
      attachments: [original],
    );
    addTearDown(draft.dispose);
    addTearDown(host.dispose);
    final sending = draft.send(7);
    draft.input.text = 'Next message';
    draft.attachments.add(newer);
    await draft.send(8);
    expect(requested, {
      'session': 'session',
      'expected_revision': 7,
      'message': {
        'text': 'Original',
        'attachments': ['original'],
      },
    });
    reply.complete({});
    await sending;
    expect(draft.input.text, 'Next message');
    expect(draft.attachments, [newer]);
    expect(draft.sending, isFalse);
  });

  test('reconciliation clears only captured composer input', () async {
    final connection = RecoveryConnection();
    var submissions = 0;
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      connection: connection,
      command: (_, _) async {
        submissions++;
        throw const CommandFailure('outcome_unknown', request: 'original');
      },
    );
    final file = attachment('file');
    final newer = attachment('newer');
    final draft = TurnDraft(
      host: host,
      session: 'session',
      text: 'Original',
      attachments: [file],
    );
    addTearDown(draft.dispose);
    addTearDown(host.dispose);
    await expectLater(draft.send(7), throwsA(isA<CommandFailure>()));
    draft.attachments.add(newer);
    expect(await draft.check(), isTrue);
    expect(draft.input.text, isEmpty);
    expect(draft.attachments, [newer]);
    expect(draft.pending, isNull);
    expect(submissions, 1);
    expect(connection.requests, ['original']);
  });

  test('artifact reconciliation retains the composer draft', () async {
    final connection = RecoveryConnection();
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      connection: connection,
      command: (_, data) async {
        expect(data!['message'], {
          'text': 'Artifact response',
          'attachments': [],
        });
        throw const CommandFailure('outcome_unknown', request: 'artifact');
      },
    );
    final file = attachment('draft-file');
    final draft = TurnDraft(
      host: host,
      session: 'session',
      text: 'Unsent draft',
      attachments: [file],
    );
    addTearDown(draft.dispose);
    addTearDown(host.dispose);
    await expectLater(
      draft.send(7, message: 'Artifact response'),
      throwsA(isA<CommandFailure>()),
    );
    expect(await draft.check(), isTrue);
    expect(connection.requests, ['artifact']);
    expect(draft.input.text, 'Unsent draft');
    expect(draft.attachments, [file]);
  });

  test('late send completion after disposal is ignored', () async {
    final reply = Completer<Map<String, dynamic>>();
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      command: (_, _) => reply.future,
    );
    addTearDown(host.dispose);
    final draft = TurnDraft(host: host, session: 'session', text: 'Message');
    final sending = draft.send(7);
    draft.dispose();
    reply.complete({});
    await sending;
  });
}
