import 'dart:async';
import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/attachments.dart';
import 'package:sailry_mobile/features/conversations/live/creation_draft.dart';
import 'package:sailry_mobile/features/conversations/live/history_write.dart';
import 'package:sailry_mobile/features/conversations/live/turn_draft.dart';
import 'package:sailry_mobile/runtime/session.dart';

import 'support/drafts.dart';

PickedAttachment attachment(String id) =>
    (attachment: {'id': id}, preview: null);

class MalformedStartConnection extends RecoveryConnection {
  final started = <String>[];
  final prepared = <String>[];

  @override
  Future<String> prepare({required String command}) async {
    prepared.add(command);
    return '{"id":"original"}';
  }

  @override
  Future<String> execute({required String request}) async {
    started.add(request);
    return jsonEncode({
      'Ok': {
        'kind': 'queued_turn',
        'data': {
          'session': 'unexpected',
          'id': 'turn',
          'revision': 1,
          'request': 'wrong',
        },
      },
    });
  }
}

void main() {
  // A released failure ends its request whatever its code: the request never
  // reached the Node, or the Node can never tell what it did.
  for (final (code, released, rejected) in const [
    ('revision_conflict', false, true),
    ('invalid_request', false, true),
    ('conflict', false, true),
    ('not_found', false, true),
    ('not_configured', false, true),
    ('outcome_unknown', false, false),
    ('unavailable', false, false),
    ('permission_denied', false, false),
    ('busy', false, false),
    ('busy', true, true),
    ('permission_denied', true, true),
    ('unavailable', true, true),
    ('outcome_unknown', true, true),
  ]) {
    group(released ? '$code released' : code, () {
      test('creation permits correction only after rejection', () async {
        final connection = RecoveryConnection()
          ..code = code
          ..released = released;
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
        final draft = CreationDraft(host, initialProject: 'project-a');
        addTearDown(draft.dispose);
        addTearDown(host.dispose);
        await expectLater(
          draft.send(message: 'First message'),
          throwsA(isA<CommandFailure>()),
        );
        expect(draft.uncertain, isTrue);
        expect(draft.pendingRequest, 'original');
        await expectLater(draft.check(), throwsA(isA<CommandFailure>()));
        expect(connection.requests, ['original']);
        expect(draft.uncertain, !rejected);
        expect(draft.pendingRequest, rejected ? isNull : 'original');
        if (rejected) {
          await draft.check();
          expect(submissions, 1);
          expect(connection.requests, ['original']);
        }
        await expectLater(
          draft.send(message: 'First message'),
          throwsA(isA<CommandFailure>()),
        );
        expect(submissions, rejected ? 2 : 1);
        expect(
          connection.requests,
          rejected ? ['original'] : ['original', 'original'],
        );
      });

      test('turn input survives unresolved admission', () async {
        final connection = RecoveryConnection()
          ..code = code
          ..released = released;
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
    final connection = RecoveryConnection()..result = startReceipt();
    var creates = 0;
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      snapshot: snapshot('a'),
      connection: connection,
      command: (kind, data) async {
        if (kind == 'read_session') {
          expect(data, {'session': 'created'});
          return sessionReceipt({
            'id': 'created',
            'revision': 1,
            'worktree': 'node-workspace',
          });
        }
        expect(kind, 'start_session');
        creates++;
        throw const CommandFailure('outcome_unknown', request: 'original');
      },
    );
    final draft = CreationDraft(host, initialProject: 'project-a');
    addTearDown(draft.dispose);
    addTearDown(host.dispose);
    await expectLater(
      draft.send(message: 'First message'),
      throwsA(isA<CommandFailure>()),
    );
    host.snapshot = {};
    host.notifyListeners();
    expect(draft.project!['id'], 'project-a');
    await draft.send(message: '');
    expect(creates, 1);
    expect(connection.requests, ['original']);
    expect(draft.created!['worktree'], 'node-workspace');
  });

  test('a transient read failure retries only the admitted session', () async {
    final commands = <String>[];
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      snapshot: snapshot('a')
        ..['sessions'] = [
          {'id': 'created', 'revision': 1},
        ],
      command: (kind, data) async {
        commands.add(kind);
        if (kind == 'start_session') return startReceipt(revision: 2);
        expect(kind, 'read_session');
        expect(data, {'session': 'created'});
        if (commands.length == 2) throw const CommandFailure('unavailable');
        return sessionReceipt({'id': 'created', 'revision': 2});
      },
    );
    final draft = CreationDraft.retain(host);
    addTearDown(host.dispose);
    final reading = expectLater(
      draft.send(message: 'First message'),
      throwsA(isA<CommandFailure>()),
    );
    draft.release();
    await reading;
    final reopened = CreationDraft.retain(host);
    expect(reopened.uncertain, isTrue);
    expect(reopened.canCheck, isTrue);
    expect(reopened.pendingRequest, isNull);
    await reopened.check();
    expect(reopened.created?['revision'], 2);
    expect(commands, ['start_session', 'read_session', 'read_session']);
    reopened.presented();
    reopened.release();
  });

  for (final code in ['not_found', 'permission_denied']) {
    test('a $code read ends an admitted draft without resending', () async {
      final commands = <String>[];
      final host = HostConnection.test(
        id: 'host',
        label: 'Host',
        snapshot: snapshot('a'),
        command: (kind, _) async {
          commands.add(kind);
          if (kind == 'start_session') return startReceipt();
          expect(kind, 'read_session');
          throw CommandFailure(code);
        },
      );
      final draft = CreationDraft(host);
      addTearDown(host.dispose);
      addTearDown(draft.dispose);
      await draft.send(message: 'First message');
      expect(draft.unavailable, isTrue);
      expect(draft.uncertain, isFalse);
      expect(draft.canCheck, isFalse);
      await draft.send(message: 'Must not be sent');
      expect(commands, ['start_session', 'read_session']);
    });
  }

  test('a mismatched start receipt retains the original request', () async {
    final connection = MalformedStartConnection()..code = 'outcome_unknown';
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      snapshot: snapshot('a'),
      connection: connection,
    );
    final draft = CreationDraft(host);
    addTearDown(host.dispose);
    addTearDown(draft.dispose);
    await expectLater(
      draft.send(message: 'First message'),
      throwsA(isA<CommandFailure>()),
    );
    expect(draft.pendingRequest, '{"id":"original"}');
    expect(draft.locationEditable, isFalse);
    await expectLater(
      draft.send(message: 'Must not be sent'),
      throwsA(isA<CommandFailure>()),
    );
    expect(connection.prepared, hasLength(1));
    expect(connection.started, ['{"id":"original"}']);
    expect(connection.requests, connection.started);

    host.snapshot['sessions'] = [
      {'id': 'created', 'revision': 1},
    ];
    host.snapshot['turns'] = [
      {'id': 'other-turn', 'request': 'unrelated', 'session': 'created'},
    ];
    host.notifyListeners();
    expect(draft.created, isNull);
    host.snapshot['turns'] = [
      {'id': 'first-turn', 'request': 'original', 'session': 'created'},
    ];
    host.notifyListeners();
    expect(draft.created?['id'], 'created');
    expect(draft.canCheck, isFalse);
    expect(connection.started, hasLength(1));
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

  test('reopened conversation checks the original unfinished send', () async {
    final reply = Completer<Map<String, dynamic>>();
    final connection = RecoveryConnection();
    var submissions = 0;
    final host = HostConnection.test(
      id: 'entry/host',
      label: 'Host',
      connection: connection,
      command: (kind, _) {
        expect(kind, 'submit_turn');
        submissions++;
        return reply.future;
      },
    );
    addTearDown(host.dispose);
    final draft = TurnDraft.retain(
      host: host,
      session: 'session',
      text: 'Message',
    );
    final failed = expectLater(draft.send(7), throwsA(isA<CommandFailure>()));
    draft.release();
    final reopened = TurnDraft.retain(host: host, session: 'session');
    expect(reopened.sending, isTrue);
    await reopened.send(8);
    reopened.release();
    reply.completeError(
      const CommandFailure('outcome_unknown', request: 'original'),
    );
    await failed;

    final pending = TurnDraft.retain(host: host, session: 'session');
    expect(pending.input.text, 'Message');
    expect(pending.pending?.request, 'original');
    expect(await pending.check(), isTrue);
    expect(connection.requests, ['original']);
    expect(submissions, 1);
    pending.release();

    final clean = TurnDraft.retain(host: host, session: 'session');
    expect(clean.input.text, isEmpty);
    expect(clean.pending, isNull);
    clean.release();
  });

  test('reopened creation checks the original unfinished admission', () async {
    final reply = Completer<Map<String, dynamic>>();
    final connection = RecoveryConnection()..result = startReceipt();
    final commands = <String>[];
    final host = HostConnection.test(
      id: 'entry/target',
      label: 'Host',
      snapshot: snapshot('a'),
      connection: connection,
      command: (kind, data) {
        commands.add(kind);
        if (kind == 'start_session') return reply.future;
        expect(kind, 'read_session');
        expect(data, {'session': 'created'});
        return Future.value(sessionReceipt({'id': 'created', 'revision': 1}));
      },
    );
    final other = HostConnection.test(id: 'other/target', label: 'Other');
    final replacement = HostConnection.test(id: host.id, label: 'Replacement');
    addTearDown(host.dispose);
    addTearDown(other.dispose);
    addTearDown(replacement.dispose);

    final draft = CreationDraft.retain(host, initialProject: 'project-a');
    draft.input.text = 'Original';
    draft.files.add(attachment('file'));
    final rejected = expectLater(
      draft.send(message: draft.input.text, attachments: ['file']),
      throwsA(isA<CommandFailure>()),
    );
    draft.release();
    final reopened = CreationDraft.retain(host);
    expect(reopened.busy, isTrue);
    await reopened.check();
    reopened.release();
    reply.completeError(
      const CommandFailure('outcome_unknown', request: 'creation-original'),
    );
    await rejected;

    for (final separate in [other, replacement]) {
      final clean = CreationDraft.retain(separate);
      expect(clean.canCheck, isFalse);
      clean.release();
    }
    final pending = CreationDraft.retain(host);
    expect(pending.input.text, 'Original');
    expect(pending.files.single.attachment['id'], 'file');
    expect(pending.project?['id'], 'project-a');
    expect(pending.canCheck, isTrue);
    expect(connection.requests, isEmpty);
    await pending.check();
    expect(connection.requests, ['creation-original']);
    expect(commands, ['start_session', 'read_session']);
    expect(pending.created?['id'], 'created');
    pending.presented();
    pending.release();
    final clean = CreationDraft.retain(host);
    expect(clean.created, isNull);
    expect(clean.input.text, isEmpty);
    clean.release();
  });

  for (final kind in ['fork_conversation', 'rewind_conversation']) {
    test('reopened conversation checks its original $kind request', () async {
      final reply = Completer<Map<String, dynamic>>();
      final connection = RecoveryConnection()..code = 'unavailable';
      final host = HostConnection.test(
        id: 'entry/target',
        label: 'Host',
        connection: connection,
        command: (command, _) {
          expect(command, kind);
          return reply.future;
        },
      );
      final other = HostConnection.test(id: 'other/target', label: 'Other');
      addTearDown(host.dispose);
      addTearDown(other.dispose);

      final first = HistoryWrite.retain(host, 'session');
      final failure = expectLater(
        first.submit(kind, {}),
        throwsA(isA<CommandFailure>()),
      );
      first.release();
      final waiting = HistoryWrite.retain(host, 'session');
      expect(waiting.busy, isTrue);
      expect(await waiting.check(), isNull);
      waiting.release();
      reply.completeError(
        const CommandFailure('outcome_unknown', request: 'history-original'),
      );
      await failure;

      for (final write in [
        HistoryWrite.retain(other, 'session'),
        HistoryWrite.retain(host, 'other-session'),
      ]) {
        expect(write.pendingKind, isNull);
        write.release();
      }
      final reopened = HistoryWrite.retain(host, 'session');
      expect(reopened.pendingKind, kind);
      expect(connection.requests, isEmpty);
      expect(await reopened.submit(kind, {}), isNull);
      await expectLater(reopened.check(), throwsA(isA<CommandFailure>()));
      expect(reopened.pendingKind, kind);

      connection.code = null;
      connection.result = {
        'data': {'id': 'resulting-session'},
      };
      final completed = (await reopened.check())!;
      expect(completed.kind, kind);
      expect(completed.output, connection.result);
      expect(reopened.consume(completed), isTrue);
      expect(await reopened.check(), isNull);
      expect(connection.requests, ['history-original', 'history-original']);
      reopened.release();
    });
  }

  test(
    'a completed fork can be opened after its original page closes',
    () async {
      final reply = Completer<Map<String, dynamic>>();
      final host = HostConnection.test(
        id: 'host',
        label: 'Host',
        command: (_, _) => reply.future,
      );
      addTearDown(host.dispose);
      final first = HistoryWrite.retain(host, 'session');
      final writing = first.submit('fork_conversation', {});
      first.release();
      reply.complete({
        'data': {'id': 'fork'},
      });
      final settled = await writing;

      final reopened = HistoryWrite.retain(host, 'session');
      expect(reopened.pendingKind, 'fork_conversation');
      expect(await reopened.check(), settled);
      expect(reopened.consume(settled!), isTrue);
      reopened.release();
    },
  );
}
