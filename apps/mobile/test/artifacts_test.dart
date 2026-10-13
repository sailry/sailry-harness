import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/artifacts.dart';
import 'package:sailry_mobile/features/conversations/live/page.dart';
import 'package:sailry_mobile/features/conversations/live/timeline.dart';
import 'package:sailry_mobile/features/resources/artifact/content.dart';
import 'package:sailry_mobile/features/resources/artifact/page.dart';
import 'package:sailry_mobile/features/resources/artifact/submission.dart';
import 'package:sailry_mobile/features/resources/artifact/surface.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:webview_flutter/webview_flutter.dart' show WebViewWidget;
import 'package:webview_flutter_platform_interface/webview_flutter_platform_interface.dart';

import 'live_conversations_test.dart' as fixture;
import 'support/webview.dart';

Map<String, dynamic> summary(int revision, {String kind = 'document'}) => {
  'id': 'artifact',
  'kind': kind,
  'session': 'session',
  'turn': 'turn',
  'package': {'name': 'owner', 'digest': 'frozen', 'settings_revision': 0},
  'title': 'Architecture',
  'summary': 'Shared resources',
  'revision': revision,
  'size': 100,
  'created_at': 1,
  'updated_at': revision,
};
Map<String, dynamic> content(int revision, {int? latest}) => {
  'artifact': summary(revision),
  'latest_revision': latest ?? revision,
  'html': '<h1>Version $revision</h1>',
  'warnings': <String>[],
  'view': {
    'template':
        '<html><head></head><body><main id="brief"></main></body></html>',
    'styles': ['main{color:var(--document-foreground)}'],
    'scripts': [
      'window.sailryDocument={mount:()=>{},setWritable:()=>{},retheme:()=>{},submitted:()=>{},readonlyExport:()=>"<main><p>Exported</p></main>"}',
    ],
    'libraries': <String>[],
  },
};
Map<String, dynamic> publication(int revision, {String kind = 'document'}) => {
  'turn': 'turn',
  'source': {'entry': 'tool-$revision', 'index': 0},
  'content': {
    'version': 1,
    'blocks': [
      {'kind': 'artifact', 'artifact': summary(revision, kind: kind)},
    ],
  },
};
Map<String, dynamic> submission(int revision) => {
  'kind': 'submit',
  'artifact': 'artifact',
  'session': 'session',
  'revision': revision,
  'form': 'choices',
  'fields': [
    {
      'name': 'storage',
      'label': 'Storage',
      'values': [
        {'label': 'Node', 'value': 'node'},
      ],
    },
  ],
};

class ArtifactPlatform extends BrowserPlatform {
  final controllers = <ArtifactController>[];
  @override
  PlatformWebViewController createPlatformWebViewController(
    PlatformWebViewControllerCreationParams params,
  ) {
    final next = ArtifactController(params, this);
    controllers.add(next);
    return controller = next;
  }
}

class ArtifactController extends BrowserController {
  ArtifactController(super.params, this.owner);
  final ArtifactPlatform owner;
  JavaScriptChannelParams? channel;
  String html = '';
  final scripts = <String>[];
  @override
  Future<void> setBackgroundColor(Color color) async {}
  @override
  Future<void> addJavaScriptChannel(JavaScriptChannelParams params) async =>
      channel = params;
  @override
  Future<void> removeJavaScriptChannel(String name) async => channel = null;
  @override
  Future<void> loadHtmlString(String value, {String? baseUrl}) async {
    html = value;
    event({'kind': 'ready'});
  }

  @override
  Future<void> runJavaScript(String script) async {
    scripts.add(script);
    if (script.contains('readonlyExport')) {
      event({
        'kind': 'export',
        'id': int.parse(RegExp(r'id:(\d+)').firstMatch(script)![1]!),
        'html': '<main><svg><path d="M0 0L1 1"/></svg><p>Saved</p></main>',
      });
    }
  }

  void event(Map<String, dynamic> value) =>
      channel?.onMessageReceived(JavaScriptMessage(message: jsonEncode(value)));
}

Future<void> mount(WidgetTester tester, Widget page) async {
  await tester.pumpWidget(MaterialApp(home: page));
  await tester.pumpAndSettle();
}

void main() {
  test('publications retain revisions and captured ownership', () {
    final other = publication(3);
    other['content']['blocks'][0]['artifact']['session'] = 'other';
    expect(
      publishedArtifacts(
        [publication(1), publication(1), publication(2), other],
        'turn',
        'session',
      ),
      [summary(1), summary(2)],
    );
    expect(
      publishedArtifacts([publication(1)], 'other-turn', 'session'),
      isEmpty,
    );
    expect(
      publishedArtifacts([publication(1)], 'turn', 'fork', origin: 'session'),
      [summary(1)],
    );
    expect(publishedArtifacts([publication(1)], 'turn', 'fork'), isEmpty);
    expect(
      publishedArtifacts([publication(1)], 'turn', 'fork', origin: 'other'),
      isEmpty,
    );
  });
  test(
    'publication kinds survive JSON restoration and reject invalid blocks',
    () {
      for (final kind in ['document', 'preview']) {
        final restored =
            jsonDecode(jsonEncode(publication(1, kind: kind)))
                as Map<String, dynamic>;
        expect(publishedArtifacts([restored], 'turn', 'session'), [
          summary(1, kind: kind),
        ]);
        final duplicate =
            jsonDecode(jsonEncode(restored)) as Map<String, dynamic>;
        expect(publishedArtifacts([restored, duplicate], 'turn', 'session'), [
          summary(1, kind: kind),
        ]);
        for (var change = 0; change < 7; change++) {
          final invalid =
              jsonDecode(jsonEncode(restored)) as Map<String, dynamic>;
          final block = invalid['content']['blocks'][0];
          final artifact = block['artifact'];
          switch (change) {
            case 0:
              block['kind'] = 'file';
            case 1:
              block.remove('artifact');
              block.addAll(artifact);
              block['kind'] = 'artifact';
            case 2:
              artifact['session'] = 'other';
            case 3:
              artifact['turn'] = 'other';
            case 4:
              artifact['revision'] = 0;
            case 5:
              artifact['id'] = '';
            case 6:
              block.remove('artifact');
          }
          expect(publishedArtifacts([invalid], 'turn', 'session'), isEmpty);
        }
      }
    },
  );
  test('form answers include capture and original choice values', () {
    expect(
      artifactAnswers(submission(2), summary(2), 'Architecture v2 (artifact)'),
      'Architecture v2 (artifact)\n- Storage: Node (node)',
    );
    expect(
      () => artifactAnswers(submission(1), summary(2), 'header'),
      throwsFormatException,
    );
    expect(
      () => artifactAnswers(
        {...submission(2), 'session': 'other'},
        summary(2),
        'header',
      ),
      throwsFormatException,
    );
  });
  test('export retains rendered content without executable scripts', () {
    final result = exportedArtifact(
      '<main><svg><path/></svg><script>bad()</script><p>Answer</p></main>',
      content(1),
      {'foreground': '#123456'},
      logo: '<svg id="logo"></svg>',
      qr: '<svg id="qr"></svg>',
    );
    expect(result, contains('<svg>'));
    expect(result, contains('Answer'));
    expect(result, isNot(contains('<script')));
    expect(result, contains('https://sailry.dev'));
    expect(result, contains('id="qr"'));
    expect(result, contains('--document-foreground:#123456'));
  });
  testWidgets('cards remain outside tool work and disabled offline', (
    tester,
  ) async {
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      command: (_, _) async => {},
    );
    final app = AppSession.test(hosts: [host]);
    addTearDown(app.dispose);
    await fixture.mount(
      tester,
      Scaffold(
        body: LiveTimeline(
          host: host,
          session: fixture.session(),
          command: (_, _) async => {},
          view: fixture.view(
            connected: false,
            entries: [fixture.entry('answer', 'assistant', 'Final answer')],
            calls: [publication(1), publication(2)],
          ),
        ),
      ),
      app,
    );
    expect(find.text('Architecture'), findsNWidgets(2));
    final cards = tester.widgetList<ListTile>(
      find.descendant(
        of: find.byType(PublishedArtifacts),
        matching: find.byType(ListTile),
      ),
    );
    expect(cards.every((tile) => tile.onTap == null), isTrue);
    expect(
      tester.getTopLeft(find.text('Architecture').first).dy,
      greaterThan(tester.getBottomLeft(find.text('Final answer')).dy),
    );
  });

  late ArtifactPlatform platform;
  setUp(() {
    platform = ArtifactPlatform();
    WebViewPlatform.instance = platform;
  });
  testWidgets('recoverable feedback distinguishes failures and repeats', (
    tester,
  ) async {
    await mount(
      tester,
      Scaffold(body: ArtifactSurface(content: content(1), connected: true)),
    );
    final native = platform.controllers.single;
    for (var repetition = 0; repetition < 2; repetition++) {
      for (final feedback in {
        'required': 'artifactFormRequired',
        'invalid_form': 'artifactFormInvalid',
        'render_error': 'artifactRenderFailed',
      }.entries) {
        native.event({'kind': feedback.key});
        await tester.pumpAndSettle();
        expect(find.text(tr(feedback.value)), findsOneWidget);
        expect(find.byType(WebViewWidget), findsOneWidget);
        await tester.pump(const Duration(seconds: 3));
        await tester.pumpAndSettle();
        expect(find.text(tr(feedback.value)), findsNothing);
      }
    }
    final exported = await tester
        .state<ArtifactSurfaceState>(find.byType(ArtifactSurface))
        .exportHtml();
    expect(exported, contains('Saved'));
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
  });
  testWidgets('inherited cards open pinned readonly documents', (tester) async {
    final reads = <Map<String, dynamic>?>[];
    var sent = 0;
    final actions = ArtifactActions(
      submit: (_) async => sent++,
      check: () async => true,
      writable: () => true,
      pending: () => false,
    );
    addTearDown(actions.dispose);
    final host = HostConnection.test(
      id: 'captured',
      label: 'Captured',
      command: (kind, data) async {
        if (kind == 'read_artifact') {
          reads.add(data);
          return {'data': content(1)};
        }
        expectSync(kind, 'list_artifact_revisions');
        expectSync(data!['session'], 'fork');
        return {
          'data': {
            'items': [summary(1)],
            'next_before': null,
          },
        };
      },
    );
    final app = AppSession.test(hosts: [host]);
    addTearDown(app.dispose);
    final view = fixture.view(calls: [publication(1)]);
    view['snapshot']['page']['runs'][0]['origin'] = 'session';
    await fixture.mount(
      tester,
      Scaffold(
        body: LiveTimeline(
          host: host,
          session: fixture.session(id: 'fork'),
          command: (_, _) async => {},
          view: view,
          artifactActions: actions,
        ),
      ),
      app,
    );
    await tester.tap(find.text('Architecture'));
    await tester.pumpAndSettle();
    expect(reads, [
      {'id': 'artifact', 'revision': 1, 'session': 'fork'},
    ]);
    expect(platform.controllers.single.html, contains('Version 1'));
    expect(platform.controllers.single.html, contains('"writable":false'));
    platform.controllers.single.event(submission(1));
    await tester.pumpAndSettle();
    expect(sent, 0);
    host.connected = false;
    host.notifyListeners();
    await tester.pumpAndSettle();
    host.connected = true;
    host.notifyListeners();
    await tester.pumpAndSettle();
    expect(reads.length, 1);
    expect(find.text('v1'), findsOneWidget);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
  });
  testWidgets('initially offline documents load after reconnect', (
    tester,
  ) async {
    final reads = <Map<String, dynamic>?>[];
    var listings = 0;
    final host = HostConnection.test(
      id: 'captured',
      label: 'Captured',
      connected: false,
      command: (kind, data) async {
        if (kind == 'read_artifact') {
          reads.add(data);
          return {'data': content(2)};
        }
        expectSync(kind, 'list_artifact_revisions');
        listings++;
        return {
          'data': {
            'items': [summary(2), summary(1)],
            'next_before': null,
          },
        };
      },
    );
    await tester.pumpWidget(
      MaterialApp(
        home: ArtifactPage(host: host, artifact: summary(1)),
      ),
    );
    await tester.pump();
    expect(reads, isEmpty);
    expect(listings, 0);
    expect(platform.controllers, isEmpty);
    host.connected = true;
    host.notifyListeners();
    await tester.pumpAndSettle();
    expect(reads, [
      {'id': 'artifact', 'revision': null, 'session': 'session'},
    ]);
    expect(listings, 1);
    expect(find.text('v2'), findsOneWidget);
    expect(platform.controllers.single.html, contains('Version 2'));
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
  });
  testWidgets('reconnect preserves an unloaded historical selection', (
    tester,
  ) async {
    final reads = <Map<String, dynamic>?>[];
    final host = HostConnection.test(
      id: 'captured',
      label: 'Captured',
      command: (kind, data) async {
        if (kind == 'read_artifact') {
          reads.add(data);
          if (reads.length == 1) {
            throw const CommandFailure('unavailable');
          }
          return {'data': content(data!['revision'] as int, latest: 3)};
        }
        expectSync(kind, 'list_artifact_revisions');
        return {
          'data': {
            'items': [summary(3), summary(2), summary(1)],
            'next_before': null,
          },
        };
      },
    );
    await mount(tester, ArtifactPage(host: host, artifact: summary(1)));
    expect(platform.controllers, isEmpty);
    host.connected = false;
    host.notifyListeners();
    await tester.pumpAndSettle();
    await tester.tap(find.text('v1'));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('artifact-version-2')));
    await tester.pumpAndSettle();
    expect(reads.length, 1);
    host.connected = true;
    host.notifyListeners();
    await tester.pumpAndSettle();
    expect(reads, [
      {'id': 'artifact', 'revision': null, 'session': 'session'},
      {'id': 'artifact', 'revision': 2, 'session': 'session'},
    ]);
    expect(find.text('v2'), findsOneWidget);
    expect(platform.controllers.single.html, contains('Version 2'));
    expect(platform.controllers.single.html, contains('"writable":false'));
    await tester.pump(const Duration(seconds: 3));
    await tester.pumpAndSettle();
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
  });
  testWidgets('latest revisions refresh only for artifact cursor', (
    tester,
  ) async {
    var latest = 2;
    final reads = <Map<String, dynamic>?>[];
    var listings = 0;
    final host = HostConnection.test(
      id: 'captured',
      label: 'Captured',
      command: (kind, data) async {
        if (kind == 'read_artifact') {
          reads.add(data);
          return {
            'data': content(
              data!['revision'] as int? ?? latest,
              latest: latest,
            ),
          };
        }
        expectSync(kind, 'list_artifact_revisions');
        listings++;
        return {
          'data': {
            'items': [
              for (var revision = latest; revision > 0; revision--)
                {...summary(revision), 'id': 'artifact'},
            ],
            'next_before': null,
          },
        };
      },
    );
    final changes = ValueNotifier<Map<String, dynamic>>({
      'artifact_cursor': 1,
      'snapshot': {'sequence': 1},
    });
    addTearDown(changes.dispose);
    await mount(
      tester,
      ArtifactPage(host: host, artifact: summary(1), changes: changes),
    );
    expect(reads, [
      {'id': 'artifact', 'revision': null, 'session': 'session'},
    ]);
    expect(find.text('v2'), findsOneWidget);
    final initial = listings;
    changes.value = {
      'artifact_cursor': 1,
      'snapshot': {'sequence': 400},
    };
    await tester.pump(const Duration(seconds: 1));
    expect(listings, initial);
    latest = 3;
    changes.value = {'artifact_cursor': 2};
    await tester.pump(const Duration(milliseconds: 500));
    await tester.pumpAndSettle();
    expect(reads.last, {'id': 'artifact', 'revision': 3, 'session': 'session'});
    expect(find.text('v3'), findsOneWidget);
    platform.controllers.last.event({
      'kind': 'edited',
      'artifact': 'artifact',
      'session': 'session',
      'revision': 3,
    });
    latest = 4;
    changes.value = {'artifact_cursor': 3};
    await tester.pump(const Duration(milliseconds: 500));
    await tester.pumpAndSettle();
    expect(find.text('v3'), findsOneWidget);
    expect(reads.last, {'id': 'artifact', 'revision': 3, 'session': 'session'});
    expect(
      platform.controllers.last.scripts.last,
      contains('setWritable(false)'),
    );
    final dirty = platform.controllers.last;
    final beforeReconnect = reads.length;
    host.connected = false;
    host.notifyListeners();
    await tester.pumpAndSettle();
    host.connected = true;
    host.notifyListeners();
    await tester.pumpAndSettle();
    expect(reads.length, beforeReconnect);
    expect(platform.controllers.last, same(dirty));
    expect(find.text('v3'), findsOneWidget);
    await tester.tap(find.text('v3'));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('artifact-version-1')));
    await tester.pumpAndSettle();
    expect(reads.last, {'id': 'artifact', 'revision': 1, 'session': 'session'});
    expect(find.text('v1'), findsOneWidget);
    await tester.tap(find.text('v1'));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('artifact-version-4')));
    await tester.pumpAndSettle();
    expect(reads.last, {'id': 'artifact', 'revision': 4, 'session': 'session'});
    latest = 5;
    changes.value = {'artifact_cursor': 4};
    await tester.pump(const Duration(milliseconds: 500));
    await tester.pumpAndSettle();
    expect(find.text('v4'), findsOneWidget);
    expect(reads.last, {'id': 'artifact', 'revision': 4, 'session': 'session'});
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
  });
  testWidgets('native bridge retains fields on uncertain admission', (
    tester,
  ) async {
    var pending = false;
    final sent = <String>[];
    final actions = ArtifactActions(
      submit: (message) async {
        sent.add(message);
        pending = true;
        throw const CommandFailure('outcome_unknown', request: 'same');
      },
      check: () async {
        pending = false;
        return true;
      },
      writable: () => !pending,
      pending: () => pending,
    );
    addTearDown(actions.dispose);
    await mount(
      tester,
      Scaffold(
        body: ArtifactSurface(
          content: content(2),
          connected: true,
          actions: actions,
        ),
      ),
    );
    final native = platform.controllers.single;
    native.event(submission(2));
    await tester.pumpAndSettle();
    expect(sent.single, contains('Storage: Node (node)'));
    expect(
      native.scripts.any((script) => script.contains('submitted')),
      isFalse,
    );
    expect(find.text(tr('conversationCheckResult')), findsOneWidget);
    await tester.tap(find.text(tr('conversationCheckResult')));
    await tester.pumpAndSettle();
    expect(sent.length, 1);
    expect(
      native.scripts.any(
        (script) => script.contains('submitted("choices",true)'),
      ),
      isTrue,
    );
    expect(find.text(tr('conversationCheckResult')), findsNothing);
    await tester.pump(const Duration(seconds: 3));
    await tester.pumpAndSettle();
    await tester.pumpWidget(const SizedBox());
    expect(native.channel, isNull);
  });
  testWidgets(
    'form sends reuse parent queue admission and retain composer draft',
    (tester) async {
      final connection = fixture.ConnectionFixture();
      final messages = <Map<String, dynamic>?>[];
      final host = HostConnection.test(
        id: 'node',
        label: 'Node',
        connection: connection,
        snapshot: {
          'sessions': [fixture.session()],
        },
        command: (kind, data) async {
          if (kind == 'read_artifact') return {'data': content(1)};
          if (kind == 'list_artifact_revisions') {
            return {
              'data': {
                'items': [summary(1)],
                'next_before': null,
              },
            };
          }
          expectSync(kind, 'submit_turn');
          messages.add(data);
          throw const CommandFailure('outcome_unknown', request: 'original');
        },
      );
      final app = AppSession.test(hosts: [host]);
      addTearDown(app.dispose);
      await fixture.mount(
        tester,
        LiveConversationPage(
          host: host,
          sessionId: 'session',
          initialSession: fixture.session(),
          initialDraft: 'Keep my draft',
        ),
        app,
      );
      connection.updates.emit(
        fixture.view(
          status: 'running',
          entries: [fixture.entry('answer', 'assistant', 'Answer')],
          calls: [publication(1)],
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.text('Architecture'));
      await tester.pumpAndSettle();
      platform.controllers.single.event(submission(1));
      await tester.pumpAndSettle();
      expect(messages.single!['session'], 'session');
      expect(messages.single!['expected_revision'], 7);
      expect(messages.single!['message']['attachments'], isEmpty);
      expect(
        messages.single!['message']['text'],
        contains('Storage: Node (node)'),
      );
      expect(messages.single!.keys, isNot(contains('steer')));
      await tester.tap(find.byTooltip(tr('back')));
      await tester.pumpAndSettle();
      expect(find.text('Keep my draft'), findsOneWidget);
      expect(find.text(tr('conversationCheckResult')), findsOneWidget);
      await tester.tap(find.text(tr('conversationCheckResult')));
      await tester.pumpAndSettle();
      expect(connection.reconciled, ['original']);
      expect(messages.length, 1);
      expect(find.text('Keep my draft'), findsOneWidget);
      await tester.pump(const Duration(seconds: 3));
      await tester.pumpAndSettle();
      await tester.pumpWidget(const SizedBox());
    },
  );
}
