import 'dart:convert';
import 'dart:typed_data';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_bridge/api/connection.dart';
import 'package:sailry_bridge/api/transfers/download.dart';
import 'package:sailry_mobile/content/attachment.dart';
import 'package:sailry_mobile/content/code.dart';
import 'package:sailry_mobile/content/document_images.dart';
import 'package:sailry_mobile/content/visuals.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';

class _Download extends Fake implements Download {
  _Download(this.bytes);
  Uint8List? bytes;

  @override
  Future<Uint8List?> next() async {
    final next = bytes;
    bytes = null;
    return next;
  }

  @override
  Future<void> close() async {}

  @override
  void dispose() {}
}

class _Connection extends Fake implements Connection {
  _Connection(this.bytes);
  final Uint8List bytes;
  int reads = 0;

  @override
  Future<Download> downloadFile({required String download}) async {
    reads++;
    return _Download(bytes);
  }

  @override
  Future<Download> downloadAttachment({required String download}) async =>
      _Download(bytes);
}

class _Picker extends FilePickerPlatform {
  Uint8List? saved;
  String? name;
  String? mime;

  @override
  Future<Uri?> saveFile({
    required String fileName,
    required Uint8List bytes,
    required String mimeType,
    String? dialogTitle,
    String? initialDirectory,
    Function(FilePickerStatus)? onFileSaving,
    WindowsOptions windowsOptions = const WindowsOptions(),
    LinuxOptions linuxOptions = const LinuxOptions(),
    WebOptions webOptions = const WebOptions(),
  }) async {
    saved = bytes;
    name = fileName;
    mime = mimeType;
    return Uri.file('/fixture/$fileName');
  }
}

void main() {
  test('document images stay relative to their captured worktree', () {
    for (final (reference, expected) in [
      ('../images/logo.png', 'images/logo.png'),
      ('images/a%20b.svg', 'docs/images/a b.svg'),
      ('../images/%252e%252e.png', 'images/%2e%2e.png'),
      ('../../outside.png', null),
      ('%2e%2e/%2e%2e/outside.png', null),
      ('/images/logo.png', null),
      ('%2fimages/logo.png', null),
      ('file:///project/images/logo.png', null),
      ('//other/images/logo.png', null),
      ('images/a%5cb.png', null),
      ('images/a%3ab.png', null),
      ('images/a%00b.png', null),
    ]) {
      expect(
        documentImagePath('docs/readme.md', Uri.parse(reference)),
        expected,
        reason: reference,
      );
    }
    expect(documentImagePath('../readme.md', Uri.parse('image.png')), isNull);
    expect(documentImagePath('/readme.md', Uri.parse('image.png')), isNull);
  });

  test('document image identity captures entry target and version', () {
    final first = HostConnection.test(id: 'entry-a/target', label: 'Target');
    final other = HostConnection.test(id: 'entry-c/target', label: 'Target');
    final source = DocumentImageSource(
      host: first,
      worktree: 'tree',
      path: 'docs/readme.md',
      version: 'v1',
    );
    expect(source.host, same(first));
    expect(source.host.entryId, 'entry-a');
    expect(source.host.nodeId, 'target');
    expect(source.atVersion('v2').identity, isNot(source.identity));
    expect(
      DocumentImageSource(
        host: other,
        worktree: 'tree',
        path: 'docs/readme.md',
        version: 'v1',
      ).identity,
      isNot(source.identity),
    );
    first.dispose();
    other.dispose();
  });

  testWidgets(
    'Markdown attachments render only the Mermaid fence as a diagram',
    (tester) async {
      const diagram = 'flowchart LR\n  A --> B';
      final bytes = utf8.encode(
        '```mermaid\n$diagram\n```\n\n```text\n$diagram\n```',
      );
      final attachment = {
        'id': 'document',
        'spec': {
          'name': 'guide.md',
          'media_type': 'Text/Markdown',
          'worktree': 'tree',
          'revision': 'revision',
          'size': bytes.length,
        },
      };
      final host = HostConnection.test(
        id: 'entry/node',
        label: 'Node',
        connection: _Connection(bytes),
        command: (kind, data) async {
          expect(kind, 'download_attachment');
          return {
            'data': {'stream': 'stream', 'attachment': attachment},
          };
        },
      );
      addTearDown(host.dispose);
      final requests = <VisualRequest>[];
      await tester.pumpWidget(
        MaterialApp(
          supportedLocales: AppLocalizations.supportedLocales,
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          builder: (_, child) => VisualScope(
            render: (request) async {
              requests.add(request);
              return const VisualSvg(
                source:
                    '<svg viewBox="0 0 20 10"><path d="M0 0h20v10z"/></svg>',
                width: 20,
                height: 10,
              );
            },
            child: child!,
          ),
          home: Scaffold(
            body: AttachmentView(host: host, attachment: attachment),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.text('guide.md'));
      await tester.pumpAndSettle();
      expect(requests.single.kind, VisualKind.mermaid);
      expect(requests.single.source.trimRight(), diagram);
      final code = tester.widget<CodeBlock>(find.byType(CodeBlock));
      expect(code.language, 'text');
      expect(code.text.trimRight(), diagram);
      await tester.pumpWidget(const SizedBox.shrink());
    },
  );

  for (final format in ['png', 'svg']) {
    testWidgets('a cached Markdown $format image can be saved while offline', (
      tester,
    ) async {
      final bytes = format == 'png'
          ? base64Decode(
              'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==',
            )
          : Uint8List.fromList(
              utf8.encode(
                '<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24"><path d="M0 0h24v24H0z"/></svg>',
              ),
            );
      final native = _Connection(bytes);
      final host = HostConnection.test(
        id: 'entry/node',
        label: 'Node',
        connection: native,
        command: (kind, data) async {
          expect(kind, 'download_file');
          expect(data, {'worktree': 'tree', 'path': 'images/logo.$format'});
          return {
            'data': {
              'stream': 'stream',
              'worktree': 'tree',
              'path': 'images/logo.$format',
              'size': bytes.length,
              'revision': 'fixture-revision',
              'stamp': 'fixture-stamp',
            },
          };
        },
      );
      final images = DocumentImages(
        DocumentImageSource(
          host: host,
          worktree: 'tree',
          path: 'docs/readme.md',
          version: 'document-revision',
        ),
      );
      final previousPicker = FilePickerPlatform.instance;
      final picker = _Picker();
      FilePickerPlatform.instance = picker;
      addTearDown(() => FilePickerPlatform.instance = previousPicker);
      addTearDown(host.dispose);
      addTearDown(images.dispose);
      await tester.pumpWidget(
        MaterialApp(
          locale: const Locale('en'),
          supportedLocales: AppLocalizations.supportedLocales,
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          home: Scaffold(
            body: images.image(
              Uri.parse('../images/logo.$format'),
              'Logo',
              null,
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      if (format == 'png') {
        // Codec work finishes outside the fake clock used by pumpAndSettle.
        final raster = find.byType(Image);
        final provider = tester.widget<Image>(raster).image;
        final context = tester.element(raster);
        await tester.runAsync(() => precacheImage(provider, context));
        await tester.pumpAndSettle();
      }
      expect(tester.getSize(find.byType(InkWell).first).isEmpty, isFalse);
      await tester.tap(find.byType(InkWell).first);
      await tester.pumpAndSettle();
      expect(find.text('Save'), findsOneWidget);
      host.connected = false;
      await tester.tap(find.text('Save'));
      await tester.pumpAndSettle();
      expect(picker.saved, bytes);
      expect(picker.name, 'logo.$format');
      expect(picker.mime, format == 'svg' ? 'image/svg+xml' : 'image/png');
      expect(native.reads, 1);
      await tester.pumpWidget(const SizedBox.shrink());
    });
  }
}
