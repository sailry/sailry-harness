import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:sailry_mobile/features/updates/releases.dart';
import 'package:sailry_mobile/features/updates/service.dart';

import 'support/updates.dart' as fixture;

const repository = 'sailry/sailry-code';
const prefix = 'mobile-v';

Map<String, dynamic> mobileRelease(String version) =>
    fixture.release(version, repository: repository, tagPrefix: prefix);

Map<String, dynamic> apk(Map<String, dynamic> release) =>
    (release['assets'] as List).single as Map<String, dynamic>;

AppUpdates updates(
  List<Map<String, dynamic>> releases, {
  String current = '0.2.13',
  String buildNumber = '31',
  MockClientHandler? request,
}) => AppUpdates(
  repository: repository,
  tagPrefix: prefix,
  requireBuildNumber: true,
  target: 'android-arm64',
  readInfo: () async => fixture.info(current, buildNumber: buildNumber),
  client: MockClient(
    request ?? (_) async => http.Response(jsonEncode(releases), 200),
  ),
);

void main() {
  test(
    'accepts the published GitHub URL with raw or encoded build separator',
    () async {
      for (final tag in ['mobile-v0.2.14+32', 'mobile-v0.2.14%2B32']) {
        final release = mobileRelease('0.2.14+32');
        final url =
            'https://github.com/$repository/releases/download/$tag/Sailry-android-arm64.apk';
        apk(release)['browser_download_url'] = url;
        final service = updates([release]);
        addTearDown(service.dispose);
        expect(await service.check(), UpdateResult.available);
        expect(service.available!.version, precedence('0.2.14'));
        expect(service.available!.buildNumber, 32);
        expect(service.available!.download.toString(), url);
      }
    },
  );

  test('strictly confines encoded downloads to the release asset path', () {
    const base =
        'https://github.com/$repository/releases/download/mobile-v0.2.14%2B32/Sailry-android-arm64.apk';
    for (final url in [
      base.replaceFirst('https:', 'http:'),
      base.replaceFirst('github.com', 'github.com.example'),
      base.replaceFirst('github.com', 'user@github.com'),
      base.replaceFirst('github.com', 'github.com:443'),
      '$base?download=1',
      '$base#artifact',
      base.replaceFirst('sailry-code', 'sailry-harness'),
      base.replaceFirst('mobile-v0.2.14', 'v0.2.14'),
      base.replaceFirst('%2B', '%252B'),
      base.replaceFirst('releases/download', 'releases%2Fdownload'),
      base.replaceFirst('32/Sailry', '33/Sailry'),
      '$base/extra',
      base.replaceFirst('Sailry-android', 'Other-android'),
    ]) {
      final release = mobileRelease('0.2.14+32');
      apk(release)['browser_download_url'] = url;
      expect(
        () => latestRelease(
          [release],
          current: precedence('0.2.13'),
          currentBuildNumber: 31,
          repository: repository,
          tagPrefix: prefix,
          target: 'android-arm64',
        ),
        throwsFormatException,
        reason: url,
      );
    }
  });

  test('a mobile tag without an uploaded release APK is unpublished', () async {
    final absent = mobileRelease('0.2.14+32')..['assets'] = [];
    final unfinished = mobileRelease('0.2.14+32');
    apk(unfinished)['state'] = 'starter';
    final artifactOnly = mobileRelease('0.2.14+32');
    apk(artifactOnly)['name'] = 'Sailry-android-arm64.apk.zip';
    for (final release in [absent, unfinished, artifactOnly]) {
      final service = updates([release]);
      addTearDown(service.dispose);
      expect(await service.check(), UpdateResult.unpublished);
      expect(service.available, isNull);
    }
    final empty = mobileRelease('0.2.14+32');
    apk(empty)['size'] = 0;
    final service = updates([empty]);
    addTearDown(service.dispose);
    await expectLater(service.check(), throwsFormatException);
    expect(service.available, isNull);
  });

  test(
    'desktop tags and mobile tags without install codes are not mobile updates',
    () async {
      final service = updates([
        fixture.release('9.0.0', repository: repository),
        mobileRelease('0.2.14'),
        mobileRelease('0.2.14+unknown'),
      ]);
      addTearDown(service.dispose);
      expect(await service.check(), UpdateResult.unpublished);
      expect(service.available, isNull);
    },
  );

  test(
    'requires both nondecreasing SemVer and an increasing store build',
    () async {
      for (final row in <String, UpdateResult>{
        '0.2.13+31': UpdateResult.current,
        '0.2.14+31': UpdateResult.current,
        '0.2.14+30': UpdateResult.current,
        '0.2.12+40': UpdateResult.current,
        '0.2.13+32': UpdateResult.available,
        '0.2.14+32': UpdateResult.available,
        '0.2.14-beta.1+32': UpdateResult.unpublished,
      }.entries) {
        final service = updates([mobileRelease(row.key)]);
        addTearDown(service.dispose);
        expect(await service.check(), row.value, reason: row.key);
        expect(
          service.available != null,
          row.value == UpdateResult.available,
          reason: row.key,
        );
      }
    },
  );

  test(
    'an incompatible higher release cannot mask an installable update',
    () async {
      final service = updates([
        mobileRelease('0.2.14+32'),
        mobileRelease('1.0.0+30'),
      ]);
      addTearDown(service.dispose);
      expect(await service.check(), UpdateResult.available);
      expect(service.available!.version, precedence('0.2.14'));
      expect(service.available!.buildNumber, 32);
    },
  );

  test('an invalid installed build is not reported as current', () async {
    final service = updates([], buildNumber: 'unknown');
    addTearDown(service.dispose);
    await expectLater(service.check(), throwsFormatException);
    expect(service.available, isNull);
  });

  test('removing a published APK clears the prior update', () async {
    var rows = [mobileRelease('0.2.14+32')];
    final service = updates(
      [],
      request: (request) async {
        expect(request.url.path, '/repos/$repository/releases');
        expect(request.headers['authorization'], isNull);
        return http.Response(jsonEncode(rows), 200);
      },
    );
    addTearDown(service.dispose);
    expect(await service.check(), UpdateResult.available);
    rows = [];
    expect(await service.check(), UpdateResult.unpublished);
    expect(service.available, isNull);
  });
}
