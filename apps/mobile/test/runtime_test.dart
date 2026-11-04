import 'dart:async';
import 'dart:convert';

import 'package:fake_async/fake_async.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/api/connection.dart';
import 'package:sailry_bridge/api/speech.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/runtime/speech.dart';

class Preferences extends Fake implements SharedPreferencesAsync {
  final _values = <String, Object?>{};
  bool? get enabled => _values['enabled'] as bool?;
  set enabled(bool? value) => _values['enabled'] = value;
  String? get language => _values['language'] as String?;
  set language(String? value) => _values['language'] = value;
  Future<bool?>? get pendingEnabled => _values['pending'] as Future<bool?>?;
  set pendingEnabled(Future<bool?>? value) => _values['pending'] = value;
  @override
  Future<bool?> getBool(String key) async => pendingEnabled ?? enabled;
  @override
  Future<String?> getString(String key) async => language;
  @override
  Future<void> setBool(String key, bool value) async {
    enabled = value;
  }

  @override
  Future<void> setString(String key, String value) async {
    language = value;
  }
}

class Speech extends Fake implements SpeechInput {
  int cancels = 0;
  int disposals = 0;
  int downloads = 0;
  int progressReads = 0;
  bool available = false;
  final finished = Completer<void>();
  Completer<int>? pendingProgress;
  Future<bool>? pendingReady;
  @override
  Future<bool> ready() async => pendingReady ?? available;
  @override
  Future<void> download() {
    downloads++;
    return finished.future;
  }

  @override
  Future<int> downloadProgress() {
    progressReads++;
    return pendingProgress?.future ?? Future.value(20);
  }

  @override
  Future<void> cancel() async {
    cancels++;
    if (!finished.isCompleted) finished.complete();
  }

  @override
  void dispose() {
    disposals++;
  }
}

class QuietSpeech extends SpeechController {
  QuietSpeech(super.directory) : super(preferences: Preferences());
  bool disposed = false;
  @override
  Future<void> check() async {}
  @override
  void dispose() {
    disposed = true;
    super.dispose();
  }
}

class Subscription extends Fake implements Updates {
  int closes = 0;
  int disposals = 0;
  int reads = 0;
  String? initial;
  Object? failure;
  final stopped = Completer<String>();
  @override
  Future<String> next() {
    reads++;
    if (initial != null) {
      final result = initial!;
      initial = null;
      return Future.value(result);
    }
    if (failure != null) return Future.error(failure!);
    return stopped.future;
  }

  @override
  Future<void> close() async {
    closes++;
    // A normal state lets an already waiting next call observe the closed owner.
    if (!stopped.isCompleted) stopped.complete('{}');
  }

  @override
  void dispose() {
    disposals++;
  }
}

class Channel extends Fake implements Connection {
  int closes = 0;
  int disposals = 0;
  int watches = 0;
  final subscription = Subscription();
  Future<Updates>? pendingWatch;
  Future<void>? pendingClose;
  final closeStarted = Completer<void>();
  final requests = <String>[];
  bool uncertain = false;
  String? response;
  @override
  Future<Updates> watch() async {
    watches++;
    return pendingWatch ?? subscription;
  }

  @override
  Future<String> prepare({required String command}) async => command;
  @override
  Future<String> execute({required String request}) async {
    requests.add(request);
    if (uncertain) throw StateError('response was lost');
    if (response != null) return response!;
    final kind = (jsonDecode(request) as Map)['kind'];
    return jsonEncode({
      'Ok': {
        'kind': kind == 'inspect_host' ? 'host_info' : 'host_metrics',
        'data': kind == 'inspect_host'
            ? {'name': 'Paired Node'}
            : <String, dynamic>{},
      },
    });
  }

  @override
  Future<void> close() async {
    closes++;
    if (!closeStarted.isCompleted) closeStarted.complete();
    await pendingClose;
  }

  @override
  void dispose() {
    disposals++;
  }
}

class Owner extends Fake implements Controller {
  int closes = 0;
  int disposals = 0;
  final addresses = <String>[];
  Future<List<String>>? pendingPeers;
  Future<Connection> Function(String)? connecting;
  String ticket = '';
  String? pin;
  String? origin;
  String? pairingError;
  Future<String>? pendingPair;
  final pairStarted = Completer<void>();
  final forgotten = <String>[];
  Future<void>? pendingForget;
  final forgetStarted = Completer<void>();
  @override
  Future<List<String>> peers() async => pendingPeers ?? addresses;
  @override
  Future<Connection> connect({required String address}) => connecting!(address);
  @override
  Future<String> pair({required String ticket}) async {
    this.ticket = ticket;
    if (!pairStarted.isCompleted) pairStarted.complete();
    return pendingPair ?? address('paired');
  }

  @override
  Future<String> pairPin({required String origin, required String pin}) async {
    this.origin = origin;
    this.pin = pin;
    if (pairingError != null) throw StateError(pairingError!);
    if (!pairStarted.isCompleted) pairStarted.complete();
    return pendingPair ?? address('paired');
  }

  @override
  Future<void> forget({required String address}) async {
    forgotten.add(address);
    if (!forgetStarted.isCompleted) forgetStarted.complete();
    await pendingForget;
    addresses.remove(address);
  }

  @override
  Future<void> close() async {
    closes++;
  }

  @override
  void dispose() {
    disposals++;
  }
}

String address(String id) => jsonEncode({'id': id});

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  group('host removal', () {
    test("removes hosts without losing selection or subscriptions", () async {
      for (final (caseName, run) in <(String, Future<void> Function(String))>[
        (
          "forgets offline hosts and releases subscriptions",
          (caseName) async {
            final owner = Owner();
            final firstChannel = Channel();
            final secondChannel = Channel();
            final snapshot = <String, dynamic>{
              'sessions': [
                {'id': 'saved-session'},
              ],
              'projects': [
                {'id': 'saved-project'},
              ],
            };
            final first = HostConnection.test(
              id: 'first',
              label: 'Offline host',
              address: address('first'),
              connected: false,
              snapshot: snapshot,
              connection: firstChannel,
            );
            final second = HostConnection.test(
              id: 'second',
              label: 'Other host',
              address: address('second'),
              connection: secondChannel,
            );
            final session = AppSession.test(
              hosts: [first, second],
              controller: owner,
            );
            final watching = first.watch();
            await Future<void>.value();
            await session.removeHost(first);
            await watching;
            expect(owner.forgotten, [address('first')], reason: caseName);
            expect(session.hosts, [second], reason: caseName);
            expect(session.selectedHost, same(second), reason: caseName);
            expect(firstChannel.subscription.closes, 1, reason: caseName);
            expect(firstChannel.subscription.disposals, 1, reason: caseName);
            expect(firstChannel.closes, 1, reason: caseName);
            expect(firstChannel.disposals, 1, reason: caseName);
            expect(firstChannel.requests, isEmpty, reason: caseName);
            expect(first.snapshot, same(snapshot), reason: caseName);
            expect(secondChannel.closes, 0, reason: caseName);
            await session.removeHost(second);
            expect(session.hosts, isEmpty, reason: caseName);
            expect(session.selectedHost, isNull, reason: caseName);
            expect(session.controller, same(owner), reason: caseName);
            expect(session.ready, isTrue, reason: caseName);
            expect(owner.closes, 0, reason: caseName);
            await session.close();
            session.dispose();
            expect(firstChannel.closes, 1, reason: caseName);
            expect(secondChannel.closes, 1, reason: caseName);
          },
        ),
        (
          "coalesces requests and preserves changed selection",
          (caseName) async {
            final pending = Completer<void>();
            final owner = Owner()..pendingForget = pending.future;
            final channel = Channel();
            final first = HostConnection.test(
              id: 'first',
              label: 'First',
              address: address('first'),
              connection: channel,
            );
            final second = HostConnection.test(id: 'second', label: 'Second');
            final session = AppSession.test(
              hosts: [first, second],
              controller: owner,
            );
            final removing = session.removeHost(first);
            final repeated = session.removeHost(first);
            await owner.forgetStarted.future;
            expect(owner.forgotten, [address('first')], reason: caseName);
            expect(session.hosts, [first, second], reason: caseName);
            expect(channel.closes, 0, reason: caseName);
            session.selectHost(second.id);
            pending.complete();
            await Future.wait([removing, repeated]);
            expect(session.hosts, [second], reason: caseName);
            expect(session.selectedHost, same(second), reason: caseName);
            expect(channel.closes, 1, reason: caseName);
            await session.close();
            session.dispose();
          },
        ),
      ]) {
        await run(caseName);
      }
    });

    test('retains host when persistence fails', () async {
      final pending = Completer<void>();
      final owner = Owner()..pendingForget = pending.future;
      final channel = Channel();
      final host = HostConnection.test(
        id: 'host',
        label: 'Host',
        address: address('host'),
        connection: channel,
      );
      final session = AppSession.test(hosts: [host], controller: owner);
      final watching = host.watch();
      await Future<void>.value();
      final removing = session.removeHost(host);
      final rejected = expectLater(removing, throwsStateError);
      await owner.forgetStarted.future;
      pending.completeError(StateError('pairing storage failed'));
      await rejected;
      expect(session.hosts, [host]);
      expect(session.selectedHost, same(host));
      expect(channel.closes, 0);
      expect(channel.subscription.closes, 0);
      owner.pendingForget = null;
      await session.removeHost(host);
      await watching;
      expect(session.hosts, isEmpty);
      expect(owner.forgotten, [address('host'), address('host')]);
      expect(channel.closes, 1);
      await session.close();
      session.dispose();
    });

    test("serializes pairing removal and handle cleanup", () async {
      for (final (caseName, run) in <(String, Future<void> Function(String))>[
        (
          "pairing waits for pending removal",
          (caseName) async {
            for (final usePin in [false, true]) {
              final pending = Completer<void>();
              final previous = Channel();
              final replacement = Channel();
              final owner = Owner()
                ..pendingForget = pending.future
                ..connecting = (_) async => replacement;
              final host = HostConnection.test(
                id: 'paired',
                label: 'Host',
                address: address('paired'),
                connection: previous,
              );
              final session = AppSession.test(hosts: [host], controller: owner);
              final removing = session.removeHost(host);
              await owner.forgetStarted.future;
              final pairing = usePin
                  ? session.pairPin('123456')
                  : session.pair('invitation');
              await Future<void>.value();
              expect(owner.pairStarted.isCompleted, isFalse, reason: caseName);
              expect(session.hosts, [host], reason: caseName);
              pending.complete();
              await removing;
              final paired = await pairing;
              expect(paired, isNot(same(host)), reason: caseName);
              expect(paired.connection, same(replacement), reason: caseName);
              expect(session.hosts, [paired], reason: caseName);
              expect(session.selectedHost, same(paired), reason: caseName);
              expect(previous.closes, 1, reason: caseName);
              expect(replacement.closes, 0, reason: caseName);
              await session.close();
              session.dispose();
            }
          },
        ),
        (
          "pairing waits for previous handle cleanup",
          (caseName) async {
            final pending = Completer<void>();
            final previous = Channel()..pendingClose = pending.future;
            final replacement = Channel();
            final owner = Owner()..connecting = (_) async => replacement;
            final host = HostConnection.test(
              id: 'paired',
              label: 'Host',
              address: address('paired'),
              connection: previous,
            );
            final session = AppSession.test(hosts: [host], controller: owner);
            final removing = session.removeHost(host);
            await previous.closeStarted.future;
            expect(session.hosts, isEmpty, reason: caseName);
            final pairing = session.pairPin('123456');
            expect(session.removeHost(host), same(removing), reason: caseName);
            await Future<void>.value();
            expect(owner.pairStarted.isCompleted, isFalse, reason: caseName);
            pending.complete();
            await removing;
            final paired = await pairing;
            await session.removeHost(host);
            expect(session.hosts, [paired], reason: caseName);
            expect(replacement.closes, 0, reason: caseName);
            expect(owner.forgotten, [address('paired')], reason: caseName);
            await session.removeHost(paired);
            expect(owner.forgotten, [
              address('paired'),
              address('paired'),
            ], reason: caseName);
            expect(session.hosts, isEmpty, reason: caseName);
            expect(session.selectedHost, isNull, reason: caseName);
            expect(replacement.closes, 1, reason: caseName);
            await session.close();
            session.dispose();
          },
        ),
        (
          "removal follows an active pairing",
          (caseName) async {
            final pending = Completer<String>();
            final channel = Channel();
            final owner = Owner()..pendingPair = pending.future;
            final host = HostConnection.test(
              id: 'paired',
              label: 'Host',
              address: address('paired'),
              connection: channel,
            );
            final session = AppSession.test(hosts: [host], controller: owner);
            final pairing = session.pair('invitation');
            await owner.pairStarted.future;
            final removing = session.removeHost(host);
            await Future<void>.value();
            expect(owner.forgotten, isEmpty, reason: caseName);
            pending.complete(address('paired'));
            expect(await pairing, same(host), reason: caseName);
            await removing;
            expect(session.hosts, isEmpty, reason: caseName);
            expect(owner.forgotten, [address('paired')], reason: caseName);
            expect(channel.closes, 1, reason: caseName);
            await session.close();
            session.dispose();
          },
        ),
      ]) {
        await run(caseName);
      }
    });

    test('closing during removal releases each handle once', () async {
      final pending = Completer<void>();
      final owner = Owner()..pendingForget = pending.future;
      final channel = Channel();
      final host = HostConnection.test(
        id: 'host',
        label: 'Host',
        address: address('host'),
        connection: channel,
      );
      final session = AppSession.test(hosts: [host], controller: owner);
      final removing = session.removeHost(host);
      await owner.forgetStarted.future;
      await session.close();
      pending.complete();
      await removing;
      expect(session.hosts, isEmpty);
      expect(session.selectedHost, isNull);
      expect(channel.closes, 1);
      expect(channel.disposals, 1);
      expect(owner.closes, 1);
      session.dispose();
    });
  });

  test("releases failed and late startup handles", () async {
    for (final (caseName, run) in <(String, Future<void> Function(String))>[
      (
        "startup failure releases hosts and speech",
        (caseName) async {
          final old = Channel();
          final fresh = Channel();
          final failed = Owner()
            ..addresses.addAll([address('first'), address('second')]);
          failed.connecting = (value) async {
            if (value == address('second')) throw StateError('peer failed');
            return old;
          };
          final successful = Owner()
            ..addresses.add(address('first'))
            ..connecting = (_) async => fresh;
          final speech = <QuietSpeech>[];
          var attempts = 0;
          final session = AppSession.test(
            ready: false,
            initialize: () async {},
            openController: (_, _) async =>
                attempts++ == 0 ? failed : successful,
            createSpeech: (path) {
              final value = QuietSpeech(path);
              speech.add(value);
              return value;
            },
          );
          await session.start(path: '/isolated/controller');
          expect(session.ready, isFalse, reason: caseName);
          expect(session.error, isNotNull, reason: caseName);
          expect(session.hosts, isEmpty, reason: caseName);
          expect(session.controller, isNull, reason: caseName);
          expect(old.closes, 1, reason: caseName);
          expect(old.disposals, 1, reason: caseName);
          expect(failed.closes, 1, reason: caseName);
          expect(failed.disposals, 1, reason: caseName);
          expect(speech.single.disposed, isTrue, reason: caseName);
          await session.start(path: '/isolated/controller');
          expect(session.ready, isTrue, reason: caseName);
          expect(session.error, isNull, reason: caseName);
          expect(
            session.hosts.single.connection,
            same(fresh),
            reason: caseName,
          );
          await session.close();
          session.dispose();
          expect(fresh.closes, 1, reason: caseName);
          expect(fresh.disposals, 1, reason: caseName);
          expect(successful.closes, 1, reason: caseName);
          expect(speech.last.disposed, isTrue, reason: caseName);
        },
      ),
      (
        "close releases a late controller",
        (caseName) async {
          final entered = Completer<void>();
          final opening = Completer<Controller>();
          final owner = Owner();
          final session = AppSession.test(
            ready: false,
            initialize: () async {},
            openController: (_, _) {
              entered.complete();
              return opening.future;
            },
          );
          final starting = session.start(path: '/isolated/controller');
          await entered.future;
          await session.close();
          opening.complete(owner);
          await starting;
          expect(owner.closes, 1, reason: caseName);
          expect(owner.disposals, 1, reason: caseName);
          expect(session.ready, isFalse, reason: caseName);
          expect(session.hosts, isEmpty, reason: caseName);
          session.dispose();
        },
      ),
      (
        "close during peer loading prevents readiness",
        (caseName) async {
          final peers = Completer<List<String>>();
          final owner = Owner()..pendingPeers = peers.future;
          final entered = Completer<void>();
          final session = AppSession.test(
            ready: false,
            initialize: () async {},
            openController: (_, _) async => owner,
            createSpeech: (path) {
              entered.complete();
              return QuietSpeech(path);
            },
          );
          final starting = session.start(path: '/isolated/controller');
          await entered.future;
          await session.close();
          peers.complete([]);
          await starting;
          expect(session.ready, isFalse, reason: caseName);
          expect(session.controller, isNull, reason: caseName);
          expect(owner.closes, 1, reason: caseName);
          expect(owner.disposals, 1, reason: caseName);
          session.dispose();
        },
      ),
      (
        "close releases a late subscription",
        (caseName) async {
          final opening = Completer<Updates>();
          final channel = Channel()..pendingWatch = opening.future;
          final host = HostConnection.test(
            id: 'node',
            label: 'Node',
            connection: channel,
            command: (_, _) async => {},
          );
          final watching = host.watch();
          await host.close();
          opening.complete(channel.subscription);
          await watching;
          expect(channel.closes, 1, reason: caseName);
          expect(channel.disposals, 1, reason: caseName);
          expect(channel.subscription.closes, 1, reason: caseName);
          expect(channel.subscription.disposals, 1, reason: caseName);
          expect(channel.subscription.reads, 0, reason: caseName);
          host.dispose();
        },
      ),
    ]) {
      await run(caseName);
    }
  });

  test('selection restarts a stopped watcher', () async {
    for (final selected in [false, true]) {
      final channel = Channel();
      channel.subscription.failure = 'channel closed';
      final host = HostConnection.test(
        id: 'host',
        label: 'Host',
        connection: channel,
        command: (_, _) async => {},
      );
      await host.watch();
      expect(host.connected, isFalse);
      expect(channel.watches, 1);
      final recovered = Subscription()
        ..initial = jsonEncode({
          'connected': true,
          'snapshot': {
            'sessions': [
              {'id': 'session'},
            ],
          },
        });
      channel.pendingWatch = Future.value(recovered);
      final other = HostConnection.test(id: 'other', label: 'Other');
      final session = AppSession.test(
        hosts: selected ? [host, other] : [other, host],
        controller: Owner(),
      );
      final ready = Completer<void>();
      void connected() {
        if (host.connected && !ready.isCompleted) ready.complete();
      }

      host.addListener(connected);
      session.selectHost(host.id);
      await ready.future;
      host.removeListener(connected);
      expect(session.selectedHost, same(host));
      expect(session.host(host.id), same(host));
      expect(channel.watches, 2);
      expect(host.error, isNull);
      expect(host.snapshot['sessions'], [
        {'id': 'session'},
      ]);
      session.selectHost(host.id);
      session.selectHost(host.id);
      expect(channel.watches, 2);
      await session.close();
      session.dispose();
      expect(recovered.closes, 1);
      expect(channel.closes, 1);
    }
  });

  test("preserves typed failures and uncertain requests", () async {
    for (final (caseName, run) in <(String, Future<void> Function(String))>[
      (
        "typed faults survive channel closure",
        (caseName) async {
          final fault = {
            'code': 'version_mismatch',
            'message': 'Product versions differ',
          };
          final channel = Channel();
          channel.subscription
            ..initial = jsonEncode({'connected': false, 'error': fault})
            ..failure = 'channel closed';
          final host = HostConnection.test(
            id: 'host',
            label: 'Host',
            connection: channel,
            command: (_, _) async => {},
          );
          await host.watch();
          expect(host.error, fault, reason: caseName);
          expect(channel.watches, 1, reason: caseName);
          channel.pendingWatch = Future.value(
            Subscription()..failure = 'channel closed',
          );
          await host.watch();
          expect(host.error, 'channel closed', reason: caseName);
          final invalid = const FormatException('Invalid snapshot');
          channel.pendingWatch = Future.value(
            Subscription()
              ..initial = jsonEncode({'connected': false, 'error': fault})
              ..failure = invalid,
          );
          await host.watch();
          expect(host.error, same(invalid), reason: caseName);
          expect(channel.watches, 3, reason: caseName);
          await host.close();
          host.dispose();
        },
      ),
      (
        "uncertain commands await explicit retry",
        (caseName) async {
          final channel = Channel()..uncertain = true;
          final host = HostConnection.test(
            id: 'node',
            label: 'Node',
            connection: channel,
            command: (_, _) async => {},
          );
          await expectLater(
            host.execute('stable-request'),
            throwsA(
              isA<CommandFailure>()
                  .having((error) => error.code, 'code', 'outcome_unknown')
                  .having(
                    (error) => error.request,
                    'request',
                    'stable-request',
                  ),
            ),
            reason: caseName,
          );
          expect(channel.requests, ['stable-request'], reason: caseName);
          await Future<void>.value();
          expect(channel.requests.length, 1, reason: caseName);
          await host.close();
          host.dispose();
        },
      ),
      (
        "known pre-admission faults retain their code",
        (caseName) async {
          for (final code in ['unavailable', 'invalid_request']) {
            final channel = Channel()
              ..response = jsonEncode({
                'Err': {'code': code, 'message': 'Not sent'},
              });
            final host = HostConnection.test(
              id: 'node',
              label: 'Node',
              connection: channel,
              command: (_, _) async => {},
            );
            await expectLater(
              host.execute('stable-request'),
              throwsA(
                isA<CommandFailure>()
                    .having((error) => error.code, 'code', code)
                    .having(
                      (error) => error.request,
                      'request',
                      'stable-request',
                    ),
              ),
              reason: caseName,
            );
            expect(channel.requests, ['stable-request'], reason: caseName);
            await host.close();
            host.dispose();
          }
        },
      ),
    ]) {
      await run(caseName);
    }
  });

  test(
    "coordinates speech initialization cancellation and preferences",
    () async {
      for (final (caseName, run) in <(String, Future<void> Function(String))>[
        (
          "shared speech initialization and late disposal",
          (caseName) async {
            final opening = Completer<SpeechInput>();
            final entered = Completer<void>();
            final input = Speech();
            var opens = 0;
            final speech = SpeechController(
              '/isolated/speech',
              preferences: Preferences(),
              open: (_) {
                opens++;
                if (!entered.isCompleted) entered.complete();
                return opening.future;
              },
            );
            final checking = speech.check();
            final downloading = speech.download();
            await entered.future;
            speech.dispose();
            opening.complete(input);
            await Future.wait([checking, downloading]);
            expect(opens, 1, reason: caseName);
            expect(input.cancels, 1, reason: caseName);
            expect(input.disposals, 1, reason: caseName);
            expect(input.downloads, 0, reason: caseName);
          },
        ),
        (
          "early speech cancellation prevents download",
          (caseName) async {
            final opening = Completer<SpeechInput>();
            final input = Speech();
            final speech = SpeechController(
              '/isolated/speech',
              preferences: Preferences(),
              open: (_) => opening.future,
            );
            final downloading = speech.download();
            await speech.cancel();
            opening.complete(input);
            await downloading;
            expect(input.downloads, 0, reason: caseName);
            expect(speech.downloading, isFalse, reason: caseName);
            speech.dispose();
            await Future<void>.value();
          },
        ),
        (
          "speech preferences resist stale reads",
          (caseName) async {
            final loading = Completer<bool?>();
            final preferences = Preferences()..pendingEnabled = loading.future;
            final speech = SpeechController(
              '/isolated/speech',
              preferences: preferences,
              open: (_) async => Speech(),
            );
            final checking = speech.check();
            await speech.setEnabled(false);
            loading.complete(true);
            await checking;
            expect(speech.enabled, isFalse, reason: caseName);
            expect(preferences.enabled, isFalse, reason: caseName);
            speech.dispose();
            await Future<void>.value();
          },
        ),
      ]) {
        await run(caseName);
      }
    },
  );

  test('bounded speech failures stop on disposal', () {
    fakeAsync((clock) {
      final input = Speech()..pendingProgress = Completer<int>();
      final speech = SpeechController(
        '/isolated/speech',
        preferences: Preferences(),
        open: (_) async => input,
      );
      var notifications = 0;
      var finished = false;
      speech.addListener(() => notifications++);
      speech.download().then((_) => finished = true);
      clock.flushMicrotasks();
      clock.elapse(const Duration(milliseconds: 250));
      expect(input.progressReads, 1);
      clock.elapse(const Duration(milliseconds: 500));
      expect(input.progressReads, 1);
      input.pendingProgress!.completeError(StateError('progress unavailable'));
      clock.flushMicrotasks();
      expect(speech.error, isNotNull);
      input.pendingProgress = Completer<int>();
      clock.elapse(const Duration(milliseconds: 250));
      speech.dispose();
      final count = notifications;
      input.pendingProgress!.complete(90);
      clock.flushMicrotasks();
      clock.elapse(const Duration(seconds: 1));
      expect(finished, isTrue);
      expect(notifications, count);
      expect(input.cancels, 1);
      expect(input.disposals, 1);
      expect(input.progressReads, 2);
    });
  });

  test("pairs trusted peers and recovers subscription failures", () async {
    for (final (caseName, run) in <(String, Future<void> Function(String))>[
      (
        "pairing trims input and reuses trusted peers",
        (caseName) async {
          final channel = Channel();
          var connections = 0;
          final owner = Owner()
            ..connecting = (_) async {
              connections++;
              return channel;
            };
          final session = AppSession.test(controller: owner);
          final first = await session.pair('  invitation  ');
          expect(owner.ticket, 'invitation', reason: caseName);
          final repeated = await session.pairPin(' 012345 ');
          expect(owner.pin, '012345', reason: caseName);
          expect(owner.origin, '', reason: caseName);
          expect(repeated, same(first), reason: caseName);
          expect(session.hosts.single, same(first), reason: caseName);
          expect(session.selectedHost, same(first), reason: caseName);
          expect(connections, 1, reason: caseName);
          await session.close();
          session.dispose();
          expect(channel.closes, 1, reason: caseName);
          expect(channel.disposals, 1, reason: caseName);
        },
      ),
      (
        "pairing restarts an ended subscription",
        (caseName) async {
          final opening = Completer<Updates>();
          final channel = Channel()..pendingWatch = opening.future;
          final owner = Owner()..connecting = (_) async => channel;
          final session = AppSession.test(controller: owner);
          final host = await session.pair('invitation');
          opening.completeError(StateError('subscription ended'));
          await Future<void>.value();
          expect(host.connected, isFalse, reason: caseName);
          expect(host.error, isNotNull, reason: caseName);
          expect(channel.watches, 1, reason: caseName);
          channel.pendingWatch = null;
          channel.subscription.initial = jsonEncode({
            'connected': true,
            'snapshot': {'sessions': []},
          });
          expect(await session.pairPin('123456'), same(host), reason: caseName);
          await Future<void>.value();
          expect(channel.watches, 2, reason: caseName);
          expect(host.connected, isTrue, reason: caseName);
          expect(host.error, isNull, reason: caseName);
          expect(await session.pairPin('123456'), same(host), reason: caseName);
          expect(channel.watches, 2, reason: caseName);
          await session.close();
          session.dispose();
        },
      ),
      (
        "pairing failures propagate without registering a peer",
        (caseName) async {
          final owner = Owner();
          final session = AppSession.test(controller: owner);
          for (final message in [
            'Unavailable: certificate validation failed',
            'NotFound: pairing code is unavailable',
          ]) {
            owner.pairingError = message;
            await expectLater(
              session.pairPin(' 123456 '),
              throwsA(
                isA<StateError>().having(
                  (error) => error.message,
                  'message',
                  message,
                ),
              ),
              reason: caseName,
            );
            expect(owner.pin, '123456', reason: caseName);
            expect(session.hosts, isEmpty, reason: caseName);
          }
          await session.close();
          session.dispose();
        },
      ),
    ]) {
      await run(caseName);
    }
  });
}
