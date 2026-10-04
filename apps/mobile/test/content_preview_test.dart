import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/content/image_preview.dart';
import 'package:sailry_mobile/content/paths.dart';
import 'package:sailry_mobile/content/markdown.dart';
import 'package:sailry_mobile/features/conversations/live/page.dart';
import 'package:sailry_mobile/features/resources/files_page.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/theme.dart';

import 'attachment_preview_test.dart' show Connection, png;
import 'live_conversations_test.dart' as chat;
import 'live_resources_test.dart' as files;

Future<void> decode(WidgetTester tester) async {
  final image = find.byType(Image).last;
  await tester.runAsync(
    () =>
        precacheImage(tester.widget<Image>(image).image, tester.element(image)),
  );
  await tester.pumpAndSettle();
}

void main() {
  test('file links strip line suffixes without changing literal paths', () {
    for (final source in [
      '/project/src/main.rs:42',
      'src/main.rs#L42',
      'file:///project/src/main.rs:42',
    ]) {
      expect(
        resourcePath(Uri.parse(source), '/project', lineReference: true),
        'src/main.rs',
      );
    }
    for (final source in [
      'src/main.rs:0',
      'src/main.rs#L-1',
      'src/main.rs:3#L4',
      'src/main.rs#L4294967296',
    ]) {
      expect(
        resourcePath(Uri.parse(source), '/project', lineReference: true),
        isNull,
      );
    }
    expect(
      resourcePath(Uri.parse('src/main.rs:42'), '/project'),
      'src/main.rs:42',
    );
    expect(
      resourcePath(
        Uri.parse('file:///C:/project/src/main.rs:42'),
        r'C:\project',
        lineReference: true,
      ),
      'src/main.rs',
    );
  });

  for (final brightness in Brightness.values) {
    testWidgets('lightbox fills the viewport and zooms in $brightness', (
      tester,
    ) async {
      tester.view.physicalSize = const Size(320, 740);
      tester.view.devicePixelRatio = 1;
      tester.view.padding = const FakeViewPadding(top: 36, bottom: 24);
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.view.resetPadding);
      var saves = 0;
      await tester.pumpWidget(
        MaterialApp(
          theme: SailryTheme.of(brightness),
          home: Scaffold(
            body: ImageThumbnail(
              image: MemoryImage(png),
              name: 'picture.png',
              save: () async {
                saves++;
                throw StateError('Save unavailable');
              },
            ),
          ),
        ),
      );
      await decode(tester);
      await tester.tap(find.byType(Image));
      await tester.pumpAndSettle();
      expect(tester.getSize(find.byType(Dialog)), const Size(320, 740));
      expect(
        tester.getRect(find.byTooltip(tr('close'))).top,
        greaterThanOrEqualTo(36),
      );
      final viewer = find.byType(InteractiveViewer);
      final controller = tester
          .widget<InteractiveViewer>(viewer)
          .transformationController!;
      expect(tester.getSize(viewer).height, greaterThan(600));
      await tester.tap(viewer);
      await tester.pump(const Duration(milliseconds: 80));
      await tester.tap(viewer);
      await tester.pumpAndSettle();
      expect(controller.value.getMaxScaleOnAxis(), 2);
      final transform = controller.value.clone();
      await tester.drag(viewer, const Offset(40, 20));
      await tester.pumpAndSettle();
      expect(controller.value, isNot(transform));
      await tester.tap(viewer);
      await tester.pump(const Duration(milliseconds: 80));
      await tester.tap(viewer);
      await tester.pumpAndSettle();
      expect(controller.value.getMaxScaleOnAxis(), 1);
      await tester.tap(find.text(tr('save')));
      await tester.pumpAndSettle();
      expect(saves, 1);
      expect(find.text(tr('fileSaveFailed')), findsOneWidget);
      expect(find.byType(InteractiveViewer), findsOneWidget);
      await tester.tap(find.byTooltip(tr('close')));
      await tester.pumpAndSettle();
      expect(find.byType(Dialog), findsNothing);
      await tester.tap(find.byType(Image));
      await tester.pumpAndSettle();
      await tester.binding.handlePopRoute();
      await tester.pumpAndSettle();
      expect(find.byType(Dialog), findsNothing);
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets('data images use the lightbox and malformed data stays local', (
    tester,
  ) async {
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: MarkdownContent(
            '![Image](data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=)',
          ),
        ),
      ),
    );
    await decode(tester);
    await tester.tap(find.byType(Image));
    await tester.pumpAndSettle();
    expect(find.byType(InteractiveViewer), findsOneWidget);
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: uriImage(Uri.parse('data:image/png;base64,not-base64')),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text(tr('conversationImageFailed')), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  test(
    'Markdown images resolve relative to the document within its worktree',
    () {
      for (final (source, expected) in [
        ('images/a%20b.png', 'docs/images/a b.png'),
        ('../assets/a.png', 'assets/a.png'),
        ('/project/assets/a.png', 'assets/a.png'),
        ('../../outside.png', null),
        ('file:///other/image.png', null),
      ]) {
        expect(
          resourcePath(Uri.parse(source), '/project', directory: 'docs'),
          expected,
        );
      }
    },
  );

  testWidgets(
    'file images open directly and Markdown shares rendering and editing',
    (tester) async {
      final connection = Connection();
      final requests = <(String, Map<String, dynamic>?)>[];
      final host = HostConnection.test(
        id: 'node',
        label: 'Node',
        connection: connection,
        snapshot: {
          'worktrees': [files.workspace],
        },
        command: (kind, data) async {
          requests.add((kind, data));
          switch (kind) {
            case 'list_directory':
              return {
                'data': {
                  'entries': data?['path'] == ''
                      ? [
                          {'name': 'docs', 'kind': 'directory'},
                        ]
                      : [
                          {'name': 'photo.PNG', 'kind': 'file'},
                          {'name': 'readme.markdown', 'kind': 'file'},
                        ],
                },
              };
            case 'download_file':
              return {'data': <String, dynamic>{}};
            case 'read_file':
              return {
                'data': {
                  'text':
                      '# Guide\n\n**Read me**\n\n![Photo](../assets/photo.png)',
                  'revision': 'r1',
                },
              };
            case 'write_file':
              return {
                'data': {'revision': 'r2'},
              };
            default:
              throw StateError('Unexpected command: $kind');
          }
        },
      );
      final app = AppSession.test(hosts: [host]);
      await files.mount(
        tester,
        const FilesPage(hostId: 'node', worktreeId: 'tree'),
        app,
      );
      await tester.tap(find.text('docs'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('photo.PNG'));
      await tester.pumpAndSettle();
      await decode(tester);
      expect(find.byType(InteractiveViewer), findsOneWidget);
      expect(find.byType(TextField), findsNothing);
      expect(requests.where((item) => item.$1 == 'read_file'), isEmpty);
      expect(requests.last.$1, 'download_file');
      expect(requests.last.$2, {'worktree': 'tree', 'path': 'docs/photo.PNG'});
      await tester.tap(find.byTooltip(tr('back')));
      await tester.pumpAndSettle();
      await tester.tap(find.text('readme.markdown'));
      await tester.pumpAndSettle();
      await decode(tester);
      expect(find.byType(MarkdownContent), findsOneWidget);
      expect(find.text('Guide'), findsOneWidget);
      expect(requests.last.$1, 'download_file');
      expect(requests.last.$2, {
        'worktree': 'tree',
        'path': 'assets/photo.png',
      });
      final count = connection.downloads.length;
      await tester.tap(find.byType(Image));
      await tester.pumpAndSettle();
      expect(find.byType(InteractiveViewer), findsOneWidget);
      expect(connection.downloads.length, count);
      await tester.tap(find.byTooltip(tr('close')));
      await tester.pumpAndSettle();
      await tester.ensureVisible(find.byTooltip(tr('edit')));
      await tester.tap(find.byTooltip(tr('edit')));
      await tester.pumpAndSettle();
      final input = find.byKey(const ValueKey('resource-file-input'));
      await tester.enterText(input, '# Updated\n\n**Saved**');
      await tester.pump();
      await tester.ensureVisible(find.byTooltip(tr('save')));
      await tester.tap(find.byTooltip(tr('save')));
      await tester.pumpAndSettle();
      expect(find.text('Updated'), findsOneWidget);
      expect(find.byType(MarkdownContent), findsOneWidget);
      expect(requests.last.$1, 'write_file');
      expect(requests.last.$2, {
        'worktree': 'tree',
        'path': 'docs/readme.markdown',
        'text': '# Updated\n\n**Saved**',
        'expected_revision': 'r1',
      });
      expect(connection.downloads.every((download) => download.closed), isTrue);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
      await app.close();
      app.dispose();
    },
  );

  testWidgets('short history begins under the header and growing replies follow', (
    tester,
  ) async {
    tester.view.padding = const FakeViewPadding(top: 44, bottom: 24);
    addTearDown(tester.view.resetPadding);
    final connection = chat.ConnectionFixture();
    final record = chat.session();
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      connection: connection,
      snapshot: {
        'sessions': [record],
      },
      command: (_, _) async => {},
    );
    final app = AppSession.test(hosts: [host]);
    await chat.mount(
      tester,
      LiveConversationPage(
        host: host,
        sessionId: 'session',
        initialSession: record,
      ),
      app,
    );
    final prompt = chat.entry('prompt', 'user', 'Show an image');
    connection.updates.emit(
      chat.view(
        entries: [prompt, chat.entry('answer', 'assistant', 'Here it is')],
      ),
    );
    await tester.pumpAndSettle();
    final header = tester.getRect(find.byTooltip(tr('modelPicker')));
    final top = tester.getTopLeft(find.text('Show an image')).dy;
    expect(top - header.bottom, inInclusiveRange(16, 64));
    tester.view.viewInsets = const FakeViewPadding(bottom: 280);
    addTearDown(tester.view.resetViewInsets);
    await tester.pumpAndSettle();
    expect(tester.getTopLeft(find.text('Show an image')).dy, top);
    connection.updates.emit(
      chat.view(
        entries: [
          prompt,
          chat.entry(
            'answer',
            'assistant',
            '${List.filled(45, 'A longer reply').join('\n\n')}\n\nEnd of reply',
          ),
        ],
      ),
    );
    await tester.pumpAndSettle();
    final list = tester.widget<ListView>(find.byType(ListView).first);
    expect(list.controller!.position.extentBefore, 0);
    expect(
      tester.getBottomLeft(find.text('End of reply')).dy,
      lessThan(
        tester.getTopLeft(find.byKey(const ValueKey('composer-input'))).dy,
      ),
    );
    expect(list.controller!.position.maxScrollExtent, greaterThan(0));
    list.controller!.jumpTo(200);
    await tester.pumpAndSettle();
    connection.updates.emit(
      chat.view(
        entries: [
          prompt,
          chat.entry(
            'answer',
            'assistant',
            '${List.filled(50, 'A longer reply').join('\n\n')}\n\nEnd of reply',
          ),
        ],
      ),
    );
    await tester.pumpAndSettle();
    expect(list.controller!.offset, 200);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    await app.close();
    app.dispose();
  });
}
