import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/attachments.dart';
import 'package:sailry_mobile/features/conversations/live/guidance.dart';
import 'package:sailry_mobile/features/conversations/live/turn_draft.dart';
import 'package:sailry_mobile/runtime/session.dart';

import 'support/drafts.dart';

const recipient = (turn: 'parent', revision: 4);

void main() {
  test('boundary guidance appears before admission', () async {
    final completion = Completer<Map<String, dynamic>>();
    Map<String, dynamic>? sent;
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      command: (kind, data) {
        expect(kind, 'guide_turn');
        sent = data;
        return completion.future;
      },
    );
    final file = (attachment: <String, dynamic>{'id': 'file'}, preview: null);
    final draft = TurnDraft(
      host: host,
      session: 'session',
      text: 'Focus here',
      attachments: [file],
    );
    addTearDown(draft.dispose);
    addTearDown(host.dispose);
    final sending = draft.send(9, recipient: recipient);
    expect(draft.guidance?.text, 'Focus here');
    expect(draft.guidance?.attachments, [file]);
    expect(draft.guidance?.turn, isNull);
    expect(draft.input.text, 'Focus here');
    expect(sent, {
      'recipient': 'parent',
      'expected_revision': 4,
      'message': {
        'text': 'Focus here',
        'attachments': ['file'],
      },
    });
    draft.input.text = 'Next message';
    completion.complete({
      'kind': 'queued_turn',
      'data': {'id': 'guidance'},
    });
    await sending;
    expect(draft.guidance?.turn, 'guidance');
    expect(draft.input.text, 'Next message');
    expect(draft.attachments, isEmpty);
  });

  test('observed guidance does not return after rewind', () async {
    final completion = Completer<Map<String, dynamic>>();
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      command: (kind, data) => completion.future,
    );
    final draft = TurnDraft(host: host, session: 'session', text: 'Focus');
    addTearDown(draft.dispose);
    addTearDown(host.dispose);
    final sending = draft.send(9, recipient: recipient);
    // An unadmitted preview has no identity to reconcile yet.
    draft.observe({'other': <String, dynamic>{}});
    expect(draft.guidance?.text, 'Focus');
    completion.complete({
      'data': {'id': 'guidance'},
    });
    await sending;
    draft.observe({});
    expect(draft.guidance?.turn, 'guidance');
    draft.observe({'guidance': <String, dynamic>{}});
    expect(draft.guidance, isNull);
    // A rewind removes the turn from the projection; nothing is revived.
    draft.observe({});
    expect(draft.guidance, isNull);
  });

  test('running input joins the active turn', () async {
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      command: (kind, data) async {
        expect(kind, 'guide_turn');
        expect(data?['recipient'], 'parent');
        expect(data?.containsKey('session'), isFalse);
        return {
          'data': {'id': 'guidance'},
        };
      },
    );
    final draft = TurnDraft(host: host, session: 'session', text: 'Focus here');
    addTearDown(draft.dispose);
    addTearDown(host.dispose);
    await draft.send(9, recipient: recipient);
    expect(draft.input.text, isEmpty);
  });

  test('rejected guidance retains the complete draft', () async {
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      command: (_, _) async => throw const CommandFailure('revision_conflict'),
    );
    final PickedAttachment file = (attachment: {'id': 'file'}, preview: null);
    final draft = TurnDraft(
      host: host,
      session: 'session',
      text: 'Focus here',
      attachments: [file],
    );
    addTearDown(draft.dispose);
    addTearDown(host.dispose);
    await expectLater(
      draft.send(9, recipient: recipient),
      throwsA(isA<CommandFailure>()),
    );
    expect(draft.input.text, 'Focus here');
    expect(draft.attachments, [file]);
    expect(draft.guidance, isNull);
    expect(draft.pending, isNull);
  });

  test('uncertain guidance checks the original request', () async {
    final connection = RecoveryConnection()
      ..result = {
        'data': {'id': 'guidance'},
      };
    var sends = 0;
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      connection: connection,
      command: (_, _) async {
        sends++;
        throw const CommandFailure('outcome_unknown', request: 'original');
      },
    );
    final draft = TurnDraft(host: host, session: 'session', text: 'Focus here');
    addTearDown(draft.dispose);
    addTearDown(host.dispose);
    await expectLater(
      draft.send(9, recipient: recipient),
      throwsA(isA<CommandFailure>()),
    );
    expect(draft.guidance?.text, 'Focus here');
    expect(draft.input.text, 'Focus here');
    await draft.send(10, recipient: recipient);
    expect(sends, 1);
    expect(await draft.check(), isTrue);
    expect(connection.requests, ['original']);
    expect(draft.guidance?.turn, 'guidance');
    expect(draft.input.text, isEmpty);
  });

  test('targets a running task', () {
    final page = {
      'runs': [
        {'turn': 'compaction', 'kind': 'compaction', 'status': 'running'},
        {'turn': 'stopping', 'kind': 'task', 'status': 'stopping'},
        {'turn': 'parent', 'kind': 'task', 'status': 'running'},
      ],
    };
    expect(runningRecipient(page)?['turn'], 'parent');
  });
}
