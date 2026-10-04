import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:sailry_mobile/features/resources/port_preview.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/ui/kit.dart';
import 'package:sailry_mobile/ui/theme.dart';
import 'package:webview_flutter/webview_flutter.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets(
    'native preview loads HTTP, scripts and WebSockets without duplicate loads',
    (tester) async {
      final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      final paths = <String>[];
      final sockets = <WebSocket>[];
      var echoes = 0;
      server.listen((request) async {
        paths.add(request.uri.path);
        if (request.uri.path == '/socket') {
          final socket = await WebSocketTransformer.upgrade(request);
          sockets.add(socket);
          socket.listen((value) {
            echoes++;
            socket.add(value);
          });
          return;
        }
        request.response.headers.contentType = ContentType.html;
        request.response.write(
          request.uri.path == '/'
              ? '''
<!doctype html><title>Preview fixture</title><p>Local preview</p>
<script>
fetch('/asset');
const socket = new WebSocket('ws://' + location.host + '/socket');
socket.onopen = () => socket.send('preview');
socket.onmessage = () => { document.title = 'Preview ready'; };
</script>
'''
              : 'asset',
        );
        await request.response.close();
      });
      final listening = ValueNotifier(true);
      addTearDown(() async {
        for (final socket in sockets) {
          await socket.close();
        }
        await server.close(force: true);
        listening.dispose();
      });
      await tester.pumpWidget(
        MaterialApp(
          theme: SailryTheme.of(Brightness.light),
          home: Builder(
            builder: (context) => TextButton(
              onPressed: () => pushPage(
                context,
                PortPreviewPage(
                  uri: Uri.parse('http://127.0.0.1:${server.port}'),
                  listening: listening,
                ),
              ),
              child: const Text('Open preview'),
            ),
          ),
        ),
      );
      await tester.tap(find.text('Open preview'));
      await until(tester, () async {
        if (find.byType(WebViewWidget).evaluate().isEmpty) return false;
        return await tester
                .widget<WebViewWidget>(find.byType(WebViewWidget))
                .platform
                .params
                .controller
                .getTitle() ==
            'Preview ready';
      });
      expect(paths.where((path) => path == '/'), hasLength(1));
      expect(paths.where((path) => path == '/asset'), hasLength(1));
      expect(echoes, 1);
      await tester.tap(find.byTooltip(tr('refresh')));
      await until(tester, () async => echoes == 2);
      expect(paths.where((path) => path == '/'), hasLength(2));
      expect(paths.where((path) => path == '/asset'), hasLength(2));
      listening.value = false;
      await until(
        tester,
        () async =>
            find.text(tr('resourceForwardStopped')).evaluate().isNotEmpty,
      );
      expect(find.byType(WebViewWidget), findsNothing);
      await tester.tap(find.byTooltip(tr('back')));
      await tester.pumpAndSettle();
      expect(find.text('Open preview'), findsOneWidget);
    },
  );
}

Future<void> until(WidgetTester tester, Future<bool> Function() ready) async {
  final deadline = DateTime.now().add(const Duration(seconds: 30));
  while (!await ready()) {
    if (DateTime.now().isAfter(deadline)) fail('Native preview timed out');
    await tester.pump(const Duration(milliseconds: 100));
    await Future<void>.delayed(const Duration(milliseconds: 100));
  }
  await tester.pump();
}
