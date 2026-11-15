import 'dart:async';
import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_bridge/api/connection.dart';
import 'package:sailry_mobile/features/conversations/live/attention.dart';
import 'package:sailry_mobile/runtime/session.dart';

class AttentionConnection extends Fake implements Connection {
  final checked = <String>[];
  Future<Map<String, dynamic>> Function()? response;

  @override
  Future<String> resolve({required String request}) async {
    checked.add(request);
    return jsonEncode(await (response?.call() ?? Future.value({'Ok': {}})));
  }

  @override
  Future<void> close() async {}

  @override
  void dispose() {}
}

class Fixture {
  Fixture() {
    host = HostConnection.test(
      id: 'host',
      label: 'Host',
      connection: connection,
      command: (kind, data) {
        expect(kind, 'set_session_read');
        sent.add(data!);
        return command?.call() ?? Future.value({});
      },
    );
    attention = ConversationAttention(
      host: host,
      sessionId: 'session',
      session: () => {
        'activity': {
          'attention': {'revision': revision, 'unread': unread},
        },
      },
      active: () => active,
    );
  }

  final connection = AttentionConnection();
  final sent = <Map<String, dynamic>>[];
  late final HostConnection host;
  late final ConversationAttention attention;
  Future<Map<String, dynamic>> Function()? command;
  int revision = 7;
  bool unread = true;
  bool active = true;

  void dispose() {
    attention.dispose();
    host.dispose();
  }
}

void main() {
  test('acknowledges each unread revision at most once', () async {
    final fixture = Fixture();
    addTearDown(fixture.dispose);
    fixture.unread = false;
    await fixture.attention.observe();
    expect(fixture.sent, isEmpty);

    fixture.unread = true;
    await fixture.attention.observe();
    await fixture.attention.observe();
    fixture.revision = 8;
    await fixture.attention.observe();
    await fixture.attention.observe();
    expect(fixture.sent, [
      {'session': 'session', 'expected_revision': 7, 'read': true},
      {'session': 'session', 'expected_revision': 8, 'read': true},
    ]);
  });

  test(
    'background failures stay silent without replaying a revision',
    () async {
      for (final failure in [
        const CommandFailure('unavailable'),
        const CommandFailure('revision_conflict'),
        const FormatException('Invalid response'),
      ]) {
        final fixture = Fixture()..command = () => Future.error(failure);
        addTearDown(fixture.dispose);
        await expectLater(fixture.attention.observe(), completes);
        await fixture.attention.observe();
        expect(fixture.sent, hasLength(1));
        expect(fixture.connection.checked, isEmpty);
      }
    },
  );

  test(
    'reconnection checks an uncertain original even after the state changes',
    () async {
      final fixture = Fixture()
        ..command = () => Future.error(
          const CommandFailure('outcome_unknown', request: 'original'),
        );
      addTearDown(fixture.dispose);
      await fixture.attention.observe();
      fixture
        ..revision = 8
        ..unread = false;
      fixture.host.connected = false;
      await fixture.attention.observe();
      expect(fixture.connection.checked, isEmpty);

      fixture.host.connected = true;
      for (final code in ['unavailable', 'outcome_unknown']) {
        fixture.connection.response = () async => {
          'Err': {'code': code},
        };
        await fixture.attention.observe();
      }
      fixture.connection.response = () async => {'Ok': {}};
      await fixture.attention.observe();
      expect(fixture.connection.checked, ['original', 'original', 'original']);
      expect(fixture.sent, hasLength(1));
    },
  );

  test(
    'a terminal check permits a newer revision without resending the original',
    () async {
      final fixture = Fixture();
      addTearDown(fixture.dispose);
      fixture.command = () => Future.error(
        const CommandFailure('outcome_unknown', request: 'original'),
      );
      await fixture.attention.observe();
      fixture.command = null;
      fixture.revision = 8;
      fixture.connection.response = () async => {
        'Err': {'code': 'outcome_unknown'},
        'released': true,
      };
      await fixture.attention.observe();
      await fixture.attention.observe();
      expect(fixture.connection.checked, ['original']);
      expect(fixture.sent.map((data) => data['expected_revision']), [7, 8]);
    },
  );

  test('coalesces state changes while an acknowledgement is running', () async {
    final reply = Completer<Map<String, dynamic>>();
    final fixture = Fixture()..command = () => reply.future;
    addTearDown(fixture.dispose);
    final pending = fixture.attention.observe();
    fixture.revision = 8;
    await fixture.attention.observe();
    expect(fixture.sent, hasLength(1));
    fixture.command = null;
    reply.complete({});
    await pending;
    expect(fixture.sent.map((data) => data['expected_revision']), [7, 8]);
  });

  test(
    'leaving the page prevents new requests and retains an in-flight result',
    () async {
      final reply = Completer<Map<String, dynamic>>();
      final fixture = Fixture()..command = () => reply.future;
      addTearDown(fixture.dispose);
      final pending = fixture.attention.observe();
      fixture.active = false;
      await fixture.attention.observe();
      reply.completeError(
        const CommandFailure('outcome_unknown', request: 'original'),
      );
      await pending;
      expect(fixture.sent, hasLength(1));
      expect(fixture.connection.checked, isEmpty);

      fixture.active = true;
      await fixture.attention.observe();
      expect(fixture.connection.checked, ['original']);
      expect(fixture.sent, hasLength(1));
    },
  );

  test(
    'disposing during a check cannot acknowledge a later revision',
    () async {
      final fixture = Fixture()
        ..command = () => Future.error(
          const CommandFailure('outcome_unknown', request: 'original'),
        );
      addTearDown(fixture.dispose);
      await fixture.attention.observe();
      fixture.revision = 8;
      final reply = Completer<Map<String, dynamic>>();
      fixture.connection.response = () => reply.future;
      final pending = fixture.attention.observe();
      fixture.attention.dispose();
      reply.complete({'Ok': {}});
      await pending;
      await fixture.attention.observe();
      expect(fixture.sent, hasLength(1));
      expect(fixture.connection.checked, ['original']);
    },
  );

  test(
    'reopening a session retains an acknowledgement that completed late',
    () async {
      final reply = Completer<Map<String, dynamic>>();
      final fixture = Fixture()..command = () => reply.future;
      addTearDown(fixture.dispose);
      final sending = fixture.attention.observe();
      fixture.attention.dispose();
      final reopened = ConversationAttention(
        host: fixture.host,
        sessionId: 'session',
        session: () => {
          'activity': {
            'attention': {'revision': 7, 'unread': true},
          },
        },
        active: () => true,
      );
      addTearDown(reopened.dispose);
      await reopened.observe();
      reply.completeError(
        const CommandFailure('outcome_unknown', request: 'original'),
      );
      await sending;
      await reopened.observe();
      expect(fixture.connection.checked, ['original']);
      expect(fixture.sent, hasLength(1));
    },
  );
}
