import 'dart:async';
import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_bridge/api/connection.dart';
import 'package:sailry_bridge/api/status.dart';
import 'package:sailry_mobile/features/conversations/live/status_feed.dart';
import 'package:sailry_mobile/runtime/session.dart';

Map<String, dynamic> entry({
  String name = 'fixture',
  String digest = 'captured',
  int settingsRevision = 1,
}) => {
  'package': {
    'name': name,
    'digest': digest,
    'settings_revision': settingsRevision,
  },
  'declaration': {'id': 'action', 'slot': 'status', 'kind': 'button'},
  'state': {'id': 'action', 'visible': true, 'enabled': true},
  'invoke': 'act',
};

class Frames extends Fake implements StatusUpdates {
  final _frames = StreamController<String>();
  late final _iterator = StreamIterator(_frames.stream);
  bool closed = false;
  bool disposed = false;

  void emit(Map<String, dynamic> view) => _frames.add(jsonEncode(view));

  @override
  Future<String> next() async {
    if (!await _iterator.moveNext()) throw StateError('subscription closed');
    return _iterator.current;
  }

  @override
  Future<void> close() async {
    if (closed) return;
    closed = true;
    await _iterator.cancel();
    await _frames.close();
  }

  @override
  void dispose() => disposed = true;
}

class Transport extends Fake implements Connection {
  var frames = Frames();
  final scopes = <String>[];
  final prepared = <(String, String)>[];
  final executed = <String>[];
  Future<String> Function(String)? respond;

  @override
  Future<StatusUpdates> watchStatus({required String scope}) async {
    if (scopes.isNotEmpty) frames = Frames();
    scopes.add(scope);
    return frames;
  }

  @override
  Future<String> prepareStatus({
    required String scope,
    required String entry,
  }) async {
    prepared.add((scope, entry));
    return 'request-${prepared.length}';
  }

  @override
  Future<String> execute({required String request}) async {
    executed.add(request);
    return respond == null
        ? jsonEncode({
            'Ok': {'kind': 'plugin_value', 'data': null},
          })
        : respond!(request);
  }
}

(StatusFeed, Transport) fixture() {
  final transport = Transport();
  final host = HostConnection.test(
    id: 'node',
    label: 'Node',
    connection: transport,
  )..connected = true;
  final feed = StatusFeed(host, {'worktree': 'worktree', 'id': 'session'});
  feed.value = {
    'connected': true,
    'entries': [entry()],
  };
  return (feed, transport);
}

void main() {
  test('actions retain the resolved session scope', () async {
    final (feed, transport) = fixture();
    await feed.invoke(entry());
    expect(jsonDecode(transport.prepared.single.$1), {
      'worktree': 'worktree',
      'session': 'session',
    });
    expect(jsonDecode(transport.prepared.single.$2), entry());
    expect(transport.executed, ['request-1']);
    feed.dispose();
  });

  test('uncertainty stays enabled for explicit retry', () async {
    final (feed, transport) = fixture();
    transport.respond = (_) async => jsonEncode({
      'Err': {'code': 'outcome_unknown', 'message': 'Receipt unavailable'},
    });
    await expectLater(feed.invoke(entry()), throwsA(isA<CommandFailure>()));
    expect(feed.enabled(entry()), isTrue);
    expect(transport.executed, ['request-1']);
    await Future<void>.delayed(Duration.zero);
    expect(transport.executed, ['request-1']);
    transport.respond = null;
    await feed.invoke(entry());
    expect(transport.prepared, hasLength(1));
    expect(transport.executed, ['request-1', 'request-1']);
    await feed.invoke(entry());
    expect(transport.prepared, hasLength(2));
    expect(transport.executed.last, 'request-2');
    feed.dispose();
  });

  test('only in-flight execution disables actions', () async {
    final (feed, transport) = fixture();
    final response = Completer<String>();
    transport.respond = (_) => response.future;
    final action = feed.invoke(entry());
    await Future<void>.delayed(Duration.zero);
    expect(feed.enabled(entry()), isFalse);
    await feed.invoke(entry());
    expect(transport.executed, hasLength(1));
    response.complete(
      jsonEncode({
        'Ok': {'kind': 'plugin_value'},
      }),
    );
    await action;
    expect(feed.enabled(entry()), isTrue);
    feed.dispose();
  });

  test('replaced captures cannot reuse earlier requests', () async {
    for (final replacement in [
      entry(digest: 'updated'),
      entry(name: 'another'),
      entry(settingsRevision: 2),
    ]) {
      final (feed, transport) = fixture();
      transport.respond = (_) async => jsonEncode({
        'Err': {'code': 'outcome_unknown'},
      });
      await expectLater(feed.invoke(entry()), throwsA(isA<CommandFailure>()));
      feed.value = {
        'connected': true,
        'entries': [replacement],
      };
      expect(feed.entries, hasLength(2));
      expect(feed.enabled(entry()), isTrue);
      expect(feed.retry(replacement), isFalse);
      transport.respond = null;
      await feed.invoke(replacement);
      expect(transport.prepared, hasLength(2));
      expect(transport.executed, ['request-1', 'request-2']);
      await feed.invoke(entry());
      expect(transport.prepared, hasLength(2));
      expect(transport.executed, ['request-1', 'request-2', 'request-1']);
      feed.dispose();
    }
  });

  test('uncertain actions survive unavailable fresh state', () async {
    for (final state in [
      {'id': 'action', 'visible': false, 'enabled': true},
      {'id': 'action', 'visible': true, 'enabled': false},
      null,
    ]) {
      final (feed, transport) = fixture();
      transport.respond = (_) async => jsonEncode({
        'Err': {'code': 'outcome_unknown'},
      });
      await expectLater(feed.invoke(entry()), throwsA(isA<CommandFailure>()));
      feed.value = {
        'connected': true,
        'entries': [
          if (state != null) {...entry(), 'state': state},
        ],
      };
      expect(feed.entries.single, entry());
      expect(feed.retry(feed.entries.single), isTrue);
      expect(feed.enabled(feed.entries.single), isTrue);
      await Future<void>.delayed(Duration.zero);
      expect(transport.executed, ['request-1']);
      transport.respond = null;
      await feed.invoke(feed.entries.single);
      expect(transport.prepared, hasLength(1));
      expect(transport.executed, ['request-1', 'request-1']);
      expect(feed.retry(entry()), isFalse);
      expect(feed.enabled(entry()), isFalse);
      await feed.invoke(entry());
      expect(transport.executed, hasLength(2));
      feed.dispose();
    }
  });

  test('uncertain retries wait for an idle connection', () async {
    final (feed, transport) = fixture();
    transport.respond = (_) async => jsonEncode({
      'Err': {'code': 'outcome_unknown'},
    });
    await expectLater(feed.invoke(entry()), throwsA(isA<CommandFailure>()));
    feed.host.connected = false;
    feed.value = {'connected': false, 'entries': []};
    expect(feed.entries, hasLength(1));
    expect(feed.enabled(entry()), isFalse);
    await feed.invoke(entry());
    expect(transport.executed, ['request-1']);
    feed.host.connected = true;
    feed.value = {'connected': true, 'entries': []};
    await Future<void>.delayed(Duration.zero);
    expect(transport.executed, ['request-1']);
    final response = Completer<String>();
    transport.respond = (_) => response.future;
    final retrying = feed.invoke(feed.entries.single);
    await Future<void>.delayed(Duration.zero);
    expect(feed.enabled(entry()), isFalse);
    await feed.invoke(entry());
    expect(transport.executed, ['request-1', 'request-1']);
    response.complete(
      jsonEncode({
        'Err': {'code': 'permission_denied'},
      }),
    );
    await expectLater(retrying, throwsA(isA<CommandFailure>()));
    expect(feed.entries, isEmpty);
    expect(feed.retry(entry()), isFalse);
    expect(transport.prepared, hasLength(1));
    feed.dispose();
  });

  for (final change in ['worktree', 'session', 'host']) {
    test('$change changes retire old scoped requests', () async {
      final (feed, transport) = fixture();
      final watching = feed.watch();
      await Future<void>.delayed(Duration.zero);
      final oldObserver = transport.frames;
      final session = {'worktree': 'worktree', 'id': 'session'};
      expect(identical(feed.rebind(feed.host, session), feed), isTrue);
      transport.respond = (_) async => jsonEncode({
        'Err': {'code': 'outcome_unknown'},
      });
      await expectLater(feed.invoke(entry()), throwsA(isA<CommandFailure>()));
      feed.value = {'connected': true, 'entries': []};
      final nextTransport = change == 'host' ? Transport() : transport;
      final host = change == 'host'
          ? HostConnection.test(
              id: 'another-node',
              label: 'Another Node',
              connection: nextTransport,
            )
          : feed.host;
      if (change == 'worktree') session['worktree'] = 'moved';
      if (change == 'session') session['id'] = 'another-session';
      final moved = feed.rebind(host, session);
      await watching;
      expect(identical(moved, feed), isFalse);
      expect(oldObserver.closed, isTrue);
      expect(oldObserver.disposed, isTrue);
      expect(feed.retry(entry()), isFalse);
      expect(feed.enabled(entry()), isFalse);
      expect(moved.retry(entry()), isFalse);
      expect(moved.entries, isEmpty);
      nextTransport.respond = null;
      final nextWatching = moved.watch();
      await Future<void>.delayed(Duration.zero);
      nextTransport.frames.emit({
        'connected': true,
        'entries': [entry()],
      });
      await Future<void>.delayed(Duration.zero);
      await moved.invoke(entry());
      expect(jsonDecode(nextTransport.scopes.last), {
        'worktree': session['worktree'],
        'session': session['id'],
      });
      expect(jsonDecode(nextTransport.prepared.last.$1), {
        'worktree': session['worktree'],
        'session': session['id'],
      });
      expect(nextTransport.prepared, hasLength(change == 'host' ? 1 : 2));
      moved.dispose();
      await nextWatching;
    });
  }

  test('subscription disposal closes the Rust observer', () async {
    final (feed, transport) = fixture();
    final watching = feed.watch();
    await Future<void>.delayed(Duration.zero);
    transport.frames.emit({
      'connected': true,
      'entries': [entry()],
    });
    await Future<void>.delayed(Duration.zero);
    expect(jsonDecode(transport.scopes.single), {
      'worktree': 'worktree',
      'session': 'session',
    });
    transport.frames.emit({
      'connected': false,
      'entries': [entry()],
    });
    await Future<void>.delayed(Duration.zero);
    expect(feed.entries, hasLength(1));
    expect(feed.enabled(entry()), isFalse);
    feed.dispose();
    await watching;
    expect(transport.frames.closed, isTrue);
    expect(transport.frames.disposed, isTrue);
  });
}
