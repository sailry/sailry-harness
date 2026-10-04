import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_bridge/api/transfers/download.dart';
import 'package:sailry_mobile/content/attachment.dart';
import 'package:sailry_mobile/features/conversations/live/timeline.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/theme.dart';

import 'image_reply_test.dart' show ImageDownload;
import 'live_conversations_test.dart' as fixture;

final png = base64Decode(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=',
);

class Connection extends fixture.ConnectionFixture {
  final downloads = <ImageDownload>[];
  Completer<Download>? held;

  @override
  Future<Download> downloadFile({required String download}) =>
      downloadAttachment(download: download);

  @override
  Future<Download> downloadAttachment({required String download}) async {
    final pending = held;
    if (pending != null) {
      held = null;
      return pending.future;
    }
    final transfer = ImageDownload()
      ..bytes = Uint8List.fromList([...png, downloads.length]);
    downloads.add(transfer);
    return transfer;
  }
}

class Host extends HostConnection {
  Host(Connection connection, CommandHandler command, {bool online = true})
    : super.test(
        id: 'node',
        label: 'Node',
        connection: connection,
        command: command,
        connected: online,
      );

  void online(bool value) {
    connected = value;
    notifyListeners();
  }
}

Map<String, dynamic> attachment(String id) => {
  'id': id,
  'spec': {
    'worktree': 'origin',
    'name': '$id.png',
    'media_type': 'image/png',
    'size': png.length,
    'revision': id,
  },
};

Future<void> mount(WidgetTester tester, Widget child) => tester.pumpWidget(
  MaterialApp(
    theme: SailryTheme.of(Brightness.light),
    home: Scaffold(body: SingleChildScrollView(child: child)),
  ),
);

Future<void> close(WidgetTester tester, Host host) async {
  await tester.pumpWidget(const SizedBox());
  await host.close();
  host.dispose();
}

void main() {
  // Image/cache lifecycles do not wait for an indefinitely active task shimmer.
  setUp(
    () =>
        TestWidgetsFlutterBinding.ensureInitialized()
            .platformDispatcher
            .accessibilityFeaturesTestValue = const FakeAccessibilityFeatures(
          disableAnimations: true,
        ),
  );
  tearDown(
    () => TestWidgetsFlutterBinding.ensureInitialized().platformDispatcher
        .clearAccessibilityFeaturesTestValue(),
  );
  testWidgets('sent images load inline without tapping the filename', (
    tester,
  ) async {
    final connection = Connection();
    final commands = <(String, Object?)>[];
    final host = Host(connection, (kind, data) async {
      commands.add((kind, data));
      return {'data': <String, dynamic>{}};
    });
    await mount(
      tester,
      LiveTimeline(
        host: host,
        session: fixture.session(),
        command: (_, _) async => {},
        view: fixture.view(
          entries: [
            {
              'id': 'user',
              'turn': 'turn',
              'author': 'user',
              'parts': [
                {'kind': 'attachment', 'data': attachment('sent')},
              ],
            },
          ],
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(commands.single.$1, 'download_attachment');
    expect(commands.single.$2, {'worktree': 'origin', 'attachment': 'sent'});
    expect(find.byType(Image), findsOneWidget);
    expect(find.byType(OutlinedButton), findsNothing);
    expect(connection.downloads.single.closed, isTrue);
    final preview = find.byType(Image);
    await tester.runAsync(
      () => precacheImage(
        tester.widget<Image>(preview).image,
        tester.element(preview),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text(tr('conversationImageFailed')), findsNothing);
    await tester.tap(preview);
    await tester.pumpAndSettle();
    expect(find.byType(InteractiveViewer), findsOneWidget);
    expect(commands.length, 1);
    expect(tester.takeException(), isNull);
    await close(tester, host);
  });

  testWidgets('generated images stay visible after tool and turn completion', (
    tester,
  ) async {
    final connection = Connection();
    final commands = <(String, Object?)>[];
    final host = Host(connection, (kind, data) async {
      commands.add((kind, data));
      return {'data': <String, dynamic>{}};
    });
    final images = [
      for (var i = 0; i < 2; i++)
        {
          'entry': 'result',
          'part': 0,
          'index': i,
          'attachment': attachment('generated-$i'),
        },
    ];
    Widget timeline(String status) => LiveTimeline(
      host: host,
      session: fixture.session(),
      command: (_, _) async => {},
      view: fixture.view(
        status: status,
        entries: [
          {
            'id': 'call',
            'turn': 'turn',
            'author': 'assistant',
            'parts': [
              {
                'kind': 'tool_call',
                'data': {
                  'arguments': {'prompt': 'Draw two images'},
                },
              },
            ],
          },
          {
            'id': 'result',
            'turn': 'turn',
            'author': 'tool',
            'parts': [
              {
                'kind': 'tool_result',
                'data': {
                  'result': {'text': 'Generated'},
                  'images': images,
                },
              },
            ],
          },
          fixture.entry('answer', 'assistant', 'Here are the images'),
        ],
        calls: [
          {
            'turn': 'turn',
            'name': 'generate_image',
            'state': 'returned',
            'source': {'entry': 'call', 'index': 0},
            'response': {'entry': 'result', 'index': 0},
          },
        ],
      ),
    );
    await mount(tester, timeline('running'));
    await tester.pumpAndSettle();
    expect(find.byType(Image), findsNWidgets(2));
    expect(commands.map((entry) => entry.$1), [
      'download_image',
      'download_image',
    ]);
    for (var i = 0; i < 2; i++) {
      expect(commands[i].$2, {'session': 'session', 'image': images[i]});
    }
    await mount(tester, timeline('completed'));
    await tester.pumpAndSettle();
    expect(find.text(tr('tool_generate_image')), findsNothing);
    expect(find.byType(Image), findsNWidgets(2));
    expect(commands.length, 2);
    final first = find.byType(Image).first;
    await tester.runAsync(
      () => precacheImage(
        tester.widget<Image>(first).image,
        tester.element(first),
      ),
    );
    await tester.pumpAndSettle();
    expect(tester.getSize(first).height, greaterThan(0));
    expect(
      tester.getTopLeft(first).dy,
      lessThan(tester.getTopLeft(find.text('Here are the images')).dy),
    );
    await tester.tap(first);
    await tester.pumpAndSettle();
    expect(find.byType(InteractiveViewer), findsOneWidget);
    Navigator.of(tester.element(find.byType(InteractiveViewer))).pop();
    await tester.pumpAndSettle();
    await tester.tap(find.text(tr('completed')));
    await tester.pumpAndSettle();
    await tester.tap(find.text(tr('tool_generate_image')));
    await tester.pumpAndSettle();
    expect(find.byType(Image), findsNWidgets(2));
    expect(commands.length, 2);
    expect(tester.takeException(), isNull);
    await close(tester, host);
  });

  testWidgets('explicit image references preview files without an extension', (
    tester,
  ) async {
    final connection = Connection();
    final commands = <(String, Object?)>[];
    final host = Host(connection, (kind, data) async {
      commands.add((kind, data));
      return {'data': <String, dynamic>{}};
    });
    await mount(
      tester,
      AttachmentView.file(
        host: host,
        worktree: 'origin',
        path: 'assets/preview',
      ),
    );
    await tester.pumpAndSettle();
    expect(commands.single.$1, 'download_file');
    expect(commands.single.$2, {
      'worktree': 'origin',
      'path': 'assets/preview',
    });
    expect(find.byType(Image), findsOneWidget);
    final preview = find.byType(Image);
    await tester.runAsync(
      () => precacheImage(
        tester.widget<Image>(preview).image,
        tester.element(preview),
      ),
    );
    await tester.pumpAndSettle();
    await tester.tap(preview);
    await tester.pumpAndSettle();
    expect(find.byType(InteractiveViewer), findsOneWidget);
    expect(tester.takeException(), isNull);
    await close(tester, host);
  });

  testWidgets(
    'failed image downloads retry and cached previews survive updates',
    (tester) async {
      var attempts = 0;
      final connection = Connection();
      final host = Host(connection, (_, _) async {
        if (++attempts == 1) throw StateError('offline');
        return {'data': <String, dynamic>{}};
      });
      Widget preview() =>
          AttachmentView(host: host, attachment: attachment('retry'));
      await mount(tester, preview());
      await tester.pumpAndSettle();
      expect(find.text(tr('conversationDownloadFailed')), findsOneWidget);
      await tester.tap(find.text(tr('retry')));
      await tester.pumpAndSettle();
      expect(find.byType(Image), findsOneWidget);
      expect(attempts, 2);
      await mount(tester, preview());
      await tester.pumpAndSettle();
      expect(attempts, 2);
      expect(tester.takeException(), isNull);
      await close(tester, host);
    },
  );

  testWidgets('offline images load when the host reconnects', (tester) async {
    var requests = 0;
    final connection = Connection();
    final host = Host(connection, (_, _) async {
      requests++;
      return {'data': <String, dynamic>{}};
    }, online: false);
    await mount(
      tester,
      AttachmentView(host: host, attachment: attachment('offline')),
    );
    await tester.pumpAndSettle();
    expect(requests, 0);
    host.online(true);
    await tester.pumpAndSettle();
    expect(requests, 1);
    expect(find.byType(Image), findsOneWidget);
    await close(tester, host);
  });

  testWidgets(
    'late bytes cannot replace a different image in the same element',
    (tester) async {
      final connection = Connection();
      final pending = Completer<Download>();
      connection.held = pending;
      final host = Host(
        connection,
        (_, _) async => {'data': <String, dynamic>{}},
      );
      await mount(
        tester,
        AttachmentView(host: host, attachment: attachment('old')),
      );
      await tester.pump();
      expect(find.byType(CircularProgressIndicator), findsOneWidget);
      await mount(
        tester,
        AttachmentView(host: host, attachment: attachment('new')),
      );
      await tester.pumpAndSettle();
      final current =
          (tester.widget<Image>(find.byType(Image)).image as MemoryImage).bytes;
      final stale = ImageDownload();
      pending.complete(stale);
      await tester.pumpAndSettle();
      expect(
        (tester.widget<Image>(find.byType(Image)).image as MemoryImage).bytes,
        same(current),
      );
      expect(tester.widget<Image>(find.byType(Image)).semanticLabel, 'new.png');
      expect(stale.closed, isTrue);
      expect(tester.takeException(), isNull);
      await close(tester, host);
    },
  );
}
