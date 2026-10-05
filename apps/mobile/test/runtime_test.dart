import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:pinput/pinput.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/api/connection.dart';
import 'package:sailry_bridge/api/speech.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:sailry_mobile/app.dart';
import 'package:sailry_mobile/features/resources/live_hosts.dart';
import 'package:sailry_mobile/features/resources/pair_form.dart';
import 'package:sailry_mobile/features/resources/project_form.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/runtime/speech.dart';
import 'package:sailry_mobile/ui/kit.dart';
import 'package:sailry_mobile/ui/theme.dart';

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
  final stopped = Completer<String>();
  @override
  Future<String> next() {
    reads++;
    if (initial != null) {
      final result = initial!;
      initial = null;
      return Future.value(result);
    }
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
  final subscription = Subscription();
  Future<Updates>? pendingWatch;
  final requests = <String>[];
  bool uncertain = false;
  @override
  Future<Updates> watch() async => pendingWatch ?? subscription;
  @override
  Future<String> prepare({required String command}) async => command;
  @override
  Future<String> execute({required String request}) async {
    requests.add(request);
    if (uncertain) throw StateError('response was lost');
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
  @override
  Future<List<String>> peers() async => pendingPeers ?? addresses;
  @override
  Future<Connection> connect({required String address}) => connecting!(address);
  @override
  Future<String> pair({required String ticket}) async {
    this.ticket = ticket;
    return address('paired');
  }

  @override
  Future<String> pairPin({required String origin, required String pin}) async {
    this.origin = origin;
    this.pin = pin;
    if (pairingError != null) throw StateError(pairingError!);
    return address('paired');
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

Future<void> mount(WidgetTester tester, Widget page, AppSession session) async {
  tester.view.physicalSize = const Size(390, 844);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.resetPhysicalSize);
  addTearDown(tester.view.resetDevicePixelRatio);
  await tester.pumpWidget(
    SessionScope(
      session: session,
      child: MaterialApp(theme: SailryTheme.of(Brightness.light), home: page),
    ),
  );
  await tester.pumpAndSettle();
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test('startup failure releases hosts and speech', () async {
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
      openController: (_, _) async => attempts++ == 0 ? failed : successful,
      createSpeech: (path) {
        final value = QuietSpeech(path);
        speech.add(value);
        return value;
      },
    );
    await session.start(path: '/isolated/controller');
    expect(session.ready, isFalse);
    expect(session.error, isNotNull);
    expect(session.hosts, isEmpty);
    expect(session.controller, isNull);
    expect(old.closes, 1);
    expect(old.disposals, 1);
    expect(failed.closes, 1);
    expect(failed.disposals, 1);
    expect(speech.single.disposed, isTrue);
    await session.start(path: '/isolated/controller');
    expect(session.ready, isTrue);
    expect(session.error, isNull);
    expect(session.hosts.single.connection, same(fresh));
    await session.close();
    session.dispose();
    expect(fresh.closes, 1);
    expect(fresh.disposals, 1);
    expect(successful.closes, 1);
    expect(speech.last.disposed, isTrue);
  });

  test('close releases a late controller', () async {
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
    expect(owner.closes, 1);
    expect(owner.disposals, 1);
    expect(session.ready, isFalse);
    expect(session.hosts, isEmpty);
    session.dispose();
  });

  test('close during peer loading prevents readiness', () async {
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
    expect(session.ready, isFalse);
    expect(session.controller, isNull);
    expect(owner.closes, 1);
    expect(owner.disposals, 1);
    session.dispose();
  });

  test('close releases a late subscription', () async {
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
    expect(channel.closes, 1);
    expect(channel.disposals, 1);
    expect(channel.subscription.closes, 1);
    expect(channel.subscription.disposals, 1);
    expect(channel.subscription.reads, 0);
    host.dispose();
  });

  test('uncertain commands await explicit retry', () async {
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
            .having((error) => error.request, 'request', 'stable-request'),
      ),
    );
    expect(channel.requests, ['stable-request']);
    await Future<void>.value();
    expect(channel.requests.length, 1);
    await host.close();
    host.dispose();
  });

  test('shared speech initialization and late disposal', () async {
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
    expect(opens, 1);
    expect(input.cancels, 1);
    expect(input.disposals, 1);
    expect(input.downloads, 0);
  });

  testWidgets('bounded speech failures stop on disposal', (tester) async {
    final input = Speech()..pendingProgress = Completer<int>();
    final speech = SpeechController(
      '/isolated/speech',
      preferences: Preferences(),
      open: (_) async => input,
    );
    var notifications = 0;
    speech.addListener(() => notifications++);
    final downloading = speech.download();
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 250));
    expect(input.progressReads, 1);
    await tester.pump(const Duration(milliseconds: 500));
    expect(input.progressReads, 1);
    input.pendingProgress!.completeError(StateError('progress unavailable'));
    await tester.pump();
    expect(speech.error, isNotNull);
    input.pendingProgress = Completer<int>();
    await tester.pump(const Duration(milliseconds: 250));
    speech.dispose();
    final count = notifications;
    input.pendingProgress!.complete(90);
    await downloading;
    await tester.pump(const Duration(seconds: 1));
    expect(notifications, count);
    expect(input.cancels, 1);
    expect(input.disposals, 1);
    expect(input.progressReads, 2);
    expect(tester.takeException(), isNull);
  });

  test('early speech cancellation prevents download', () async {
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
    expect(input.downloads, 0);
    expect(speech.downloading, isFalse);
    speech.dispose();
    await Future<void>.value();
  });

  test('speech preferences resist stale reads', () async {
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
    expect(speech.enabled, isFalse);
    expect(preferences.enabled, isFalse);
    speech.dispose();
    await Future<void>.value();
  });

  testWidgets('host switch ignores stale metrics', (tester) async {
    final old = Completer<Map<String, dynamic>>();
    var firstReads = 0;
    var secondReads = 0;
    final first = HostConnection.test(
      id: 'first',
      label: 'First',
      command: (_, _) async {
        firstReads++;
        return firstReads == 1
            ? old.future
            : {
                'data': {'cpu_basis_points': 3300},
              };
      },
    );
    final second = HostConnection.test(
      id: 'second',
      label: 'Second',
      command: (_, _) async {
        secondReads++;
        return {
          'data': {'cpu_basis_points': 2300},
        };
      },
    );
    final session = AppSession.test(hosts: [first, second]);
    await mount(tester, const LiveHostsPage(), session);
    expect(firstReads, 1);
    session.selectHost('second');
    await tester.pumpAndSettle();
    expect(secondReads, 1);
    expect(find.text('23%'), findsOneWidget);
    session.selectHost('first');
    await tester.pumpAndSettle();
    expect(firstReads, 2);
    expect(find.text('33%'), findsOneWidget);
    old.complete({
      'data': {'cpu_basis_points': 9000},
    });
    await tester.pumpAndSettle();
    expect(find.text('33%'), findsOneWidget);
    expect(find.text('90%'), findsNothing);
    await tester.pumpWidget(const SizedBox.shrink());
    await session.close();
    session.dispose();
    expect(tester.takeException(), isNull);
  });

  testWidgets('empty startup and paired Node admission', (tester) async {
    final channel = Channel();
    channel.subscription.initial = jsonEncode({
      'connected': true,
      'snapshot': {
        'projects': [],
        'sessions': [],
        'worktrees': [],
        'terminals': [],
      },
      'notifications': [],
    });
    final owner = Owner()..connecting = (_) async => channel;
    final session = AppSession.test(controller: owner);
    await tester.pumpWidget(SailryApp(session: session));
    await tester.pumpAndSettle();
    expect(find.text('Studio'), findsNothing);
    expect(find.text(tr('taskDone')), findsNothing);
    await tester.tap(find.byKey(const ValueKey('tab-1')));
    await tester.pumpAndSettle();
    expect(find.text(tr('hostConnectPrompt')), findsOneWidget);
    await tester.tap(find.widgetWithText(FilledButton, tr('pair')));
    // Pinput animates its focused cursor continuously while awaiting digits.
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 400));
    expect(find.byType(PairForm), findsOneWidget);
    expect(tester.widget<Pinput>(find.byType(Pinput)).length, 6);
    await tester.enterText(find.byType(EditableText), '12');
    await tester.tap(find.widgetWithText(FilledButton, tr('pairAction')));
    await tester.pump(const Duration(milliseconds: 400));
    expect(owner.pin, isNull);
    expect(find.text(tr('pairInvalid')), findsOneWidget);
    await tester.enterText(find.byType(EditableText), '012345');
    await tester.tap(find.widgetWithText(FilledButton, tr('pairAction')));
    await tester.pumpAndSettle();
    expect(owner.pin, '012345');
    expect(owner.origin, '');
    expect(owner.ticket, isEmpty);
    expect(session.hosts.map((host) => host.id), ['paired']);
    expect(find.text('Paired Node'), findsOneWidget);
    expect(find.byType(PairForm), findsNothing);
    await tester.pumpWidget(const SizedBox.shrink());
    await session.close();
    session.dispose();
    expect(tester.takeException(), isNull);
  });

  testWidgets('distinct pairing failures and expiry', (tester) async {
    final owner = Owner()
      ..pairingError = 'Unavailable: certificate validation failed';
    final session = AppSession.test(controller: owner);
    await tester.pumpWidget(SailryApp(session: session));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('tab-1')));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, tr('pair')));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 400));
    await tester.enterText(find.byType(EditableText), '123456');
    await tester.tap(find.widgetWithText(FilledButton, tr('pairAction')));
    await tester.pumpAndSettle();
    expect(find.text(tr('pairFailed')), findsOneWidget);
    expect(find.text(tr('pairExpired')), findsNothing);
    expect(
      tester.widget<Pinput>(find.byType(Pinput)).controller!.text,
      '123456',
    );
    owner.pairingError = 'NotFound: pairing code is unavailable';
    await tester.tap(find.widgetWithText(FilledButton, tr('pairAction')));
    await tester.pumpAndSettle();
    expect(find.text(tr('pairExpired')), findsOneWidget);
    expect(find.text(tr('pairFailed')), findsNothing);
    expect(session.hosts, isEmpty);
    await tester.pumpWidget(const SizedBox.shrink());
    await session.close();
    session.dispose();
  });

  testWidgets('project save preserves its host and uncertain draft', (
    tester,
  ) async {
    final result = Completer<Map<String, dynamic>>();
    final calls = <Map<String, dynamic>?>[];
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      command: (kind, data) {
        expect(kind, 'register_project');
        calls.add(data);
        return result.future;
      },
    );
    final session = AppSession.test(hosts: [host]);
    await mount(
      tester,
      Scaffold(
        body: Builder(
          builder: (context) => TextButton(
            onPressed: () => showAppSheet(
              context,
              tr('hostRegisterProject'),
              child: ProjectForm(host: host),
            ),
            child: const Text('Open'),
          ),
        ),
      ),
      session,
    );
    await tester.tap(find.text('Open'));
    await tester.pumpAndSettle();
    await tester.enterText(
      find.widgetWithText(TextField, tr('hostProjectName')),
      'Project',
    );
    await tester.enterText(
      find.widgetWithText(TextField, tr('hostProjectPath')),
      '/isolated/project',
    );
    await tester.tap(find.widgetWithText(FilledButton, tr('save')));
    await tester.pump();
    expect(
      tester
          .widget<TextField>(
            find.widgetWithText(TextField, tr('hostProjectName')),
          )
          .enabled,
      isFalse,
    );
    result.completeError(
      const CommandFailure('outcome_unknown', request: 'stable'),
    );
    await tester.pumpAndSettle();
    expect(calls.length, 1);
    expect(find.text('Project'), findsOneWidget);
    expect(find.byType(ProjectForm), findsOneWidget);
    await tester.pump(const Duration(seconds: 3));
    await tester.pumpAndSettle();
    await tester.pumpWidget(const SizedBox.shrink());
    await session.close();
    session.dispose();
    expect(tester.takeException(), isNull);
  });
}
