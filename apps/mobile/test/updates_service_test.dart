import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:sailry_mobile/features/updates/releases.dart';
import 'package:sailry_mobile/features/updates/service.dart';

import 'support/updates.dart' as fixture;

void main() {
  group('release selection', () {
    Release? select(
      List<Map<String, dynamic>> rows, {
      String current = '0.1.0-alpha.1',
      String target = 'android-arm64',
    }) => latestRelease(rows, current: releaseVersion(current), target: target);

    test('orders preview identifiers numerically', () {
      final selected = select([
        fixture.release('0.1.0-alpha.2'),
        fixture.release('0.1.0-alpha.10'),
        fixture.release('0.1.0-alpha.1'),
      ]);
      expect('${selected!.version}', '0.1.0-alpha.10');
    });

    test('preview installations can advance to beta and stable', () {
      expect(
        '${select([fixture.release('0.1.0-beta.1')])!.version}',
        '0.1.0-beta.1',
      );
      expect(
        '${select([fixture.release('0.1.0'), fixture.release('0.1.0-rc.1')])!.version}',
        '0.1.0',
      );
    });

    test('stable installations do not select previews', () {
      final selected = select([
        fixture.release('0.3.0-alpha.1'),
        fixture.release('0.2.0'),
      ], current: '0.1.0');
      expect('${selected!.version}', '0.2.0');
      expect(
        select([fixture.release('0.3.0-alpha.1')], current: '0.1.0'),
        isNull,
      );
    });

    test('ignores drafts and packages for other platforms', () {
      expect(
        select([
          fixture.release('0.2.0', draft: true),
          fixture.release('0.3.0', target: 'aarch64-apple-darwin'),
        ]),
        isNull,
      );
      expect(select([fixture.release('0.2.0')], target: 'ios'), isNull);
    });

    test('ignores build metadata for precedence', () {
      expect(
        releaseVersion('0.1.0-alpha.1+1'),
        releaseVersion('0.1.0-alpha.1+2'),
      );
    });

    test('rejects invalid metadata and an unrelated download', () {
      expect(
        () => latestRelease(
          {},
          current: releaseVersion('0.1.0'),
          target: 'android-arm64',
        ),
        throwsFormatException,
      );
      final row = fixture.release('0.2.0');
      (row['assets'] as List).first['browser_download_url'] =
          'https://example.invalid/update.apk';
      expect(() => select([row]), throwsFormatException);
    });

    test('does not select empty or unfinished uploads', () {
      final row = fixture.release('0.2.0');
      (row['assets'] as List).first['state'] = 'starter';
      expect(select([row]), isNull);
      (row['assets'] as List).first['state'] = 'uploaded';
      (row['assets'] as List).first['size'] = 0;
      expect(() => select([row]), throwsFormatException);
    });
  });

  group('update checks', () {
    test(
      'reads the public index over HTTP without an execution Node',
      () async {
        final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
        addTearDown(() => server.close(force: true));
        final requests = <HttpRequest>[];
        server.listen((request) async {
          requests.add(request);
          request.response.headers.contentType = ContentType.json;
          request.response.write(
            jsonEncode([fixture.release('0.1.0-alpha.2')]),
          );
          await request.response.close();
        });
        final service = AppUpdates(
          source: Uri.parse(
            'http://127.0.0.1:${server.port}/releases?per_page=100',
          ),
          target: 'android-arm64',
          readInfo: () async => fixture.info('0.1.0-alpha.1'),
        );
        addTearDown(service.dispose);
        expect(await service.check(), UpdateResult.available);
        expect(service.version, '0.1.0-alpha.1');
        expect('${service.available!.version}', '0.1.0-alpha.2');
        expect(requests.single.uri.queryParameters['per_page'], '100');
        expect(
          requests.single.headers.value('accept'),
          'application/vnd.github+json',
        );
        expect(requests.single.headers.value('authorization'), isNull);
      },
    );

    test('distinguishes current from unpublished mobile packages', () async {
      final current = fixture.updates(
        releases: [fixture.release('0.1.0-alpha.1')],
      );
      final absent = fixture.updates(
        releases: [fixture.release('0.2.0', target: 'macos')],
      );
      addTearDown(current.dispose);
      addTearDown(absent.dispose);
      expect(await current.check(), UpdateResult.current);
      expect(current.available, isNull);
      expect(await absent.check(), UpdateResult.unpublished);
    });

    test('does not announce an older version', () async {
      final service = fixture.updates(
        current: '0.1.0-alpha.10',
        releases: [fixture.release('0.1.0-alpha.2')],
      );
      addTearDown(service.dispose);
      expect(await service.check(), UpdateResult.current);
      expect(service.available, isNull);
    });

    test(
      'HTTP errors and malformed JSON are not reported as current',
      () async {
        for (final response in [
          http.Response('{}', 403),
          http.Response('invalid', 200),
        ]) {
          final service = fixture.updates(request: (_) async => response);
          addTearDown(service.dispose);
          await expectLater(service.check(), throwsA(isA<Exception>()));
          expect(service.checking, isFalse);
        }
      },
    );

    test(
      'coalesces concurrent checks and ignores late disposal results',
      () async {
        final response = Completer<http.Response>();
        var calls = 0;
        final service = fixture.updates(
          request: (_) {
            calls++;
            return response.future;
          },
        );
        final first = service.check();
        await Future<void>.delayed(Duration.zero);
        expect(await service.check(), isNull);
        expect(calls, 1);
        service.dispose();
        response.complete(
          http.Response(jsonEncode([fixture.release('0.2.0')]), 200),
        );
        expect(await first, isNull);
      },
    );

    test('limits a stalled check and permits a retry', () async {
      final service = AppUpdates(
        client: MockClient((_) => Completer<http.Response>().future),
        target: 'android-arm64',
        readInfo: () async => fixture.info('0.1.0-alpha.1'),
        timeout: const Duration(milliseconds: 10),
      );
      addTearDown(service.dispose);
      await expectLater(service.check(), throwsA(isA<TimeoutException>()));
      expect(service.checking, isFalse);
    });
  });
}
