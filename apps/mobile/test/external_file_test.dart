import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:open_filex/open_filex.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/content/external_file.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  late Directory temporary;
  const paths = MethodChannel('plugins.flutter.io/path_provider');
  const share = MethodChannel('dev.fluttercommunity.plus/share');
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;

  setUp(() async {
    temporary = await Directory.systemTemp.createTemp('sailry-exports-test-');
    messenger.setMockMethodCallHandler(paths, (call) async => temporary.path);
  });
  tearDown(() async {
    messenger.setMockMethodCallHandler(paths, null);
    messenger.setMockMethodCallHandler(share, null);
    await temporary.delete(recursive: true);
  });

  test(
    'exports isolate names and keep bytes available after system sharing',
    () async {
      final bytes = Uint8List.fromList([0, 1, 2, 255]);
      final first = await cacheExternalFile(bytes, 'report.pdf');
      final second = await cacheExternalFile(
        Uint8List.fromList([42]),
        'report.pdf',
      );
      expect(first.path, isNot(second.path));
      expect(await first.readAsBytes(), bytes);
      MethodCall? invocation;
      messenger.setMockMethodCallHandler(share, (call) async {
        invocation = call;
        return 'dev.fluttercommunity.plus/share/dismissed';
      });
      await shareFile(first, 'report.pdf', const Rect.fromLTWH(24, 32, 36, 36));
      expect(invocation!.method, 'share');
      expect(invocation!.arguments, {
        'paths': [first.path],
        'mimeTypes': ['application/pdf'],
        'originX': 24.0,
        'originY': 32.0,
        'originWidth': 36.0,
        'originHeight': 36.0,
      });
      expect(await first.readAsBytes(), bytes);
      final escaped = await cacheExternalFile(bytes, '../outside.pdf');
      expect(
        escaped.parent.parent.path,
        '${temporary.path}/sailry-file-exports',
      );
      expect(escaped.uri.pathSegments.last, '.._outside.pdf');
    },
  );

  test(
    'native sharing failures propagate without deleting the export',
    () async {
      final file = await cacheExternalFile(Uint8List.fromList([1]), 'note.md');
      messenger.setMockMethodCallHandler(
        share,
        (_) async => throw PlatformException(code: 'unavailable'),
      );
      await expectLater(
        shareFile(file, 'note.md', const Rect.fromLTWH(0, 0, 36, 36)),
        throwsA(isA<PlatformException>()),
      );
      expect(await file.exists(), isTrue);
    },
  );
  test(
    'external opening uses the cached path and reports missing applications',
    () async {
      final file = await cacheExternalFile(
        Uint8List.fromList([1]),
        'report.pdf',
      );
      await expectLater(
        openFile(
          file,
          'report.pdf',
          launch: (path, {type}) async {
            expect(path, file.path);
            expect(type, 'application/pdf');
            return OpenResult(type: ResultType.noAppToOpen);
          },
        ),
        throwsA(
          isA<FileOpenFailure>().having(
            (error) => error.message,
            'message',
            tr('fileNoApplication'),
          ),
        ),
      );
      await openFile(
        file,
        'archive.unknown',
        launch: (path, {type}) async {
          expect(type, isNull);
          return OpenResult();
        },
      );
      expect(await file.exists(), isTrue);
    },
  );
}
