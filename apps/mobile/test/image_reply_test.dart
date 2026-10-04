import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_bridge/api/transfers/download.dart';
import 'package:sailry_mobile/features/conversations/live/timeline.dart';
import 'package:sailry_mobile/content/paths.dart';
import 'package:sailry_mobile/content/markdown.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/theme.dart';
import 'package:sailry_mobile/l10n/strings.dart';

import 'live_conversations_test.dart' as fixture;

class ImageDownload implements Download {
  Uint8List? bytes = base64Decode(
    'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=',
  );
  bool closed = false;

  @override
  Future<Uint8List?> next() async {
    final chunk = bytes;
    bytes = null;
    return chunk;
  }

  @override
  Future<void> close() async => closed = true;
  @override
  void dispose() {}
  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

class ImageConnection extends fixture.ConnectionFixture {
  final transfer = ImageDownload();
  @override
  Future<Download> downloadFile({required String download}) async => transfer;
  @override
  Future<Download> downloadAttachment({required String download}) async =>
      transfer;
}

void main() {
  for (final (name, source, path) in [
    (
      'streamed image download link',
      '已生成图片：[下载蜡笔小新插画](assets/generated/crayon_shinchan_illustration.png)',
      'assets/generated/crayon_shinchan_illustration.png',
    ),
    (
      'reference image link',
      'Before [**Download**][picture]\n\nAfter\n\n'
          '[picture]: <file:///project/assets/a%20b(1).PNG> "Preview"',
      'assets/a b(1).PNG',
    ),
  ]) {
    testWidgets('$name previews and opens the execution file', (tester) async {
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(320, 800);
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      final connection = ImageConnection();
      final commands = <(String, Object?)>[];
      final host = HostConnection.test(
        id: 'node',
        label: 'Node',
        connection: connection,
        command: (kind, data) async {
          commands.add((kind, data));
          return {'data': <String, dynamic>{}};
        },
      );
      host.snapshot = {
        'worktrees': [
          {'id': 'original-tree', 'path': '/project'},
        ],
      };
      final view = fixture.view(
        entries: [
          {
            'id': 'reply',
            'turn': 'turn',
            'author': 'assistant',
            'parts': [
              for (final rune in source.runes)
                {'kind': 'text', 'data': String.fromCharCode(rune)},
            ],
          },
        ],
      );
      view['snapshot']['page']['runs'][0]['worktree'] = 'original-tree';
      await tester.pumpWidget(
        MaterialApp(
          theme: SailryTheme.of(Brightness.light),
          home: Scaffold(
            body: SingleChildScrollView(
              child: LiveTimeline(
                host: host,
                session: fixture.session(),
                command: (_, _) async => {},
                view: view,
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(commands.single.$1, 'download_file');
      expect(commands.single.$2, {'worktree': 'original-tree', 'path': path});
      expect(
        tester.widget<MarkdownContent>(find.byType(MarkdownContent)).data,
        source,
      );
      final preview = find.byType(Image);
      expect(preview, findsOneWidget);
      await tester.runAsync(
        () => precacheImage(
          tester.widget<Image>(preview).image,
          tester.element(preview),
        ),
      );
      await tester.pumpAndSettle();
      final before = find.text(
        name.startsWith('streamed') ? '已生成图片：' : 'Before ',
      );
      expect(
        tester.getBottomLeft(before).dy,
        lessThanOrEqualTo(tester.getTopLeft(preview).dy),
      );
      await tester.tap(preview);
      await tester.pumpAndSettle();
      expect(find.byType(InteractiveViewer), findsOneWidget);
      expect(commands.length, 1);
      expect(connection.transfer.closed, isTrue);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
      await host.close();
      host.dispose();
    });
  }

  test('image paths stay within the captured worktree', () {
    expect(resourcePath(Uri.parse('assets/a%20b.png'), null), 'assets/a b.png');
    expect(
      resourcePath(Uri.parse('file:///project/assets/a.png'), '/project'),
      'assets/a.png',
    );
    expect(resourcePath(Uri.parse('/another/a.png'), '/project'), isNull);
    expect(resourcePath(Uri.parse('../a.png'), '/project'), isNull);
    expect(resourcePath(Uri.parse('.'), '/project', allowRoot: true), '');
    expect(
      resourcePath(Uri.parse('/project'), '/project', allowRoot: true),
      '',
    );
    expect(
      resourcePath(Uri.parse('/project-other'), '/project', allowRoot: true),
      isNull,
    );
    expect(resourcePath(Uri.parse('..'), '/project', allowRoot: true), isNull);
    expect(
      resourcePath(
        Uri.parse('file://other/project'),
        '/project',
        allowRoot: true,
      ),
      isNull,
    );
    expect(resourcePath(Uri.parse('.'), '/project'), isNull);
    expect(
      resourcePath(Uri.parse('#heading'), '/project', directory: 'docs'),
      isNull,
    );
    expect(resourcePath(Uri.parse('%FF.md'), '/project'), isNull);
    expect(
      resourcePath(Uri.parse('../guide.md'), '/project', directory: 'docs'),
      'guide.md',
    );
    expect(
      resourcePath(Uri.parse('file:///C:/project/a.png'), r'C:\project'),
      'a.png',
    );
    expect(
      resourcePath(Uri.parse('file://another/project/a.png'), '/project'),
      isNull,
    );
    expect(resourcePath(Uri.parse('assets/../a.png'), '/project'), 'a.png');
  });

  for (final corrupt in [false, true]) {
    testWidgets(
      corrupt
          ? 'invalid image has a readable error'
          : 'Markdown image uses its execution worktree',
      (tester) async {
        final connection = ImageConnection();
        if (corrupt) connection.transfer.bytes = Uint8List.fromList([1, 2, 3]);
        final commands = <(String, Object?)>[];
        final host = HostConnection.test(
          id: 'node',
          label: 'Node',
          connection: connection,
          command: (kind, data) async {
            commands.add((kind, data));
            return {'data': <String, dynamic>{}};
          },
        );
        final view = fixture.view(
          entries: [
            fixture.entry(
              'reply',
              'assistant',
              'Before\n\n![Image](assets/image.png)\n\nAfter',
            ),
          ],
        );
        view['snapshot']['page']['runs'][0]['worktree'] = 'original-tree';
        await tester.pumpWidget(
          MaterialApp(
            theme: SailryTheme.of(Brightness.light),
            home: Scaffold(
              body: SingleChildScrollView(
                child: LiveTimeline(
                  host: host,
                  session: fixture.session(),
                  command: (_, _) async => {},
                  view: view,
                ),
              ),
            ),
          ),
        );
        await tester.pumpAndSettle();
        expect(commands.single.$1, 'download_file');
        expect(commands.single.$2, {
          'worktree': 'original-tree',
          'path': 'assets/image.png',
        });
        expect(connection.transfer.closed, isTrue);
        final preview = find.byType(Image);
        await tester.runAsync(
          () => precacheImage(
            tester.widget<Image>(preview).image,
            tester.element(preview),
            onError: (_, _) {},
          ),
        );
        await tester.pumpAndSettle();
        if (corrupt) {
          expect(find.text('图片无法显示'), findsOneWidget);
        } else {
          expect(
            tester.getTopLeft(find.text('Before')).dy,
            lessThan(tester.getTopLeft(preview).dy),
          );
          expect(
            tester.getBottomLeft(preview).dy,
            lessThanOrEqualTo(tester.getTopLeft(find.text('After')).dy),
          );
          await tester.tap(preview);
          await tester.pumpAndSettle();
          expect(find.byType(InteractiveViewer), findsOneWidget);
          expect(commands.length, 1);
        }
        expect(tester.takeException(), isNull);
        await tester.pumpWidget(const SizedBox());
        await host.close();
        host.dispose();
      },
    );
  }

  testWidgets('plugin image results preserve failure details', (tester) async {
    final connection = ImageConnection();
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      connection: connection,
      command: (_, _) async => {'data': <String, dynamic>{}},
    );
    final image = {
      'entry': 'result',
      'part': 0,
      'index': 0,
      'attachment': {
        'id': 'image',
        'spec': {'name': 'preview.png', 'media_type': 'image/png', 'size': 68},
      },
    };
    const diagnostic = 'provider returned HTTP 429: rate limit';
    final page = {
      'entries': [
        {
          'id': 'call',
          'parts': [
            {
              'kind': 'tool_call',
              'data': {'arguments': {}},
            },
          ],
        },
        {
          'id': 'result',
          'parts': [
            {
              'kind': 'tool_result',
              'data': {
                'result': {
                  'error': {'code': 'unavailable', 'message': diagnostic},
                },
                'images': [image],
              },
            },
          ],
        },
      ],
    };
    await tester.pumpWidget(
      MaterialApp(
        theme: SailryTheme.of(Brightness.light),
        home: Scaffold(
          body: SingleChildScrollView(
            child: LiveTimeline(
              host: host,
              session: fixture.session(),
              view: fixture.view(
                entries: [
                  for (final entry in page['entries'] as List)
                    {...entry, 'turn': 'turn', 'author': 'assistant'},
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
              command: (_, _) async => {},
            ),
          ),
        ),
      ),
    );
    await tester.tap(find.text(tr('completed')));
    await tester.pumpAndSettle();
    await tester.tap(find.text(tr('tool_generate_image')));
    await tester.pumpAndSettle();
    expect(find.byType(Image), findsOneWidget);
    expect(find.text(diagnostic), findsOneWidget);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    await host.close();
    host.dispose();
  });

  testWidgets('reply preserves text and image order', (tester) async {
    final connection = ImageConnection();
    final commands = <(String, Object?)>[];
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      connection: connection,
      command: (kind, data) async {
        commands.add((kind, data));
        return {'data': <String, dynamic>{}};
      },
    );
    final image = {
      'entry': 'answer',
      'part': 1,
      'index': 0,
      'attachment': {
        'id': 'image',
        'spec': {'name': 'image.png', 'media_type': 'image/png', 'size': 68},
      },
    };
    await tester.pumpWidget(
      MaterialApp(
        theme: SailryTheme.of(Brightness.light),
        home: Scaffold(
          body: SingleChildScrollView(
            child: LiveTimeline(
              host: host,
              session: fixture.session(),
              command: (_, _) async => {},
              view: fixture.view(
                entries: [
                  {
                    'id': 'answer',
                    'turn': 'turn',
                    'author': 'assistant',
                    'parts': [
                      {'kind': 'text', 'data': 'Before image'},
                      {'kind': 'image', 'data': image},
                      {'kind': 'text', 'data': 'After image'},
                    ],
                  },
                ],
              ),
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(commands.single.$1, 'download_image');
    expect(commands.single.$2, {'session': 'session', 'image': image});
    expect(connection.transfer.closed, isTrue);
    final preview = find.byType(Image);
    expect(preview, findsOneWidget);
    await tester.runAsync(
      () => precacheImage(
        tester.widget<Image>(preview).image,
        tester.element(preview),
      ),
    );
    await tester.pumpAndSettle();
    expect(tester.getSize(preview).height, greaterThan(0));
    expect(
      tester.getTopLeft(find.text('Before image')).dy,
      lessThan(tester.getTopLeft(preview).dy),
    );
    expect(
      tester.getBottomLeft(preview).dy,
      lessThanOrEqualTo(tester.getTopLeft(find.text('After image')).dy),
    );
    await tester.tap(preview);
    await tester.pumpAndSettle();
    expect(find.byType(InteractiveViewer), findsOneWidget);
    expect(commands.length, 1);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    await host.close();
    host.dispose();
  });
}
